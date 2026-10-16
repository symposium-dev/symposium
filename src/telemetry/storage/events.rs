//! Whole-batch encoding and append-only storage for low-volume telemetry rows.
//!
//! Batch validation and serialization finish before the event file is opened.
//! The generic handle helper in this module is a private fault-injection seam
//! for distinguishing append stages. The recorder's later RecordingWrites seam
//! operates on whole state, batch, and snapshot writes; it must not expose file
//! handles.

use std::{
    error::Error,
    fmt,
    fs::{File, OpenOptions},
    io::{self, SeekFrom},
    path::Path,
};

use crate::telemetry::schema::{LowVolumeRow, UtcDay};

use super::LockedStorage;

/// A complete append-only event batch serialized for exactly one UTC day.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct EventBatch {
    day: UtcDay,
    bytes: Box<[u8]>,
}

impl EventBatch {
    /// Validate and serialize rows in their supplied order.
    ///
    /// The authoritative day comes from the bound recording context. Checking
    /// every row against it is defense in depth; no row chooses its own file.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty batch, a row from another day, or a JSON
    /// serialization failure.
    pub(in crate::telemetry) fn build(
        day: UtcDay,
        rows: Vec<LowVolumeRow>,
    ) -> Result<Self, EventBatchError> {
        if rows.is_empty() {
            return Err(EventBatchError::Empty);
        }

        for (row_index, row) in rows.iter().enumerate() {
            let row_day = row.day();
            if row_day != day {
                return Err(EventBatchError::RowDayMismatch {
                    row_index,
                    batch_day: day,
                    row_day,
                });
            }
        }

        let mut bytes = Vec::new();
        for row in rows {
            // These rows contain only JSON-safe scalar and sequence fields, so
            // failure is defensive rather than expected in production.
            serde_json::to_writer(&mut bytes, &row).map_err(EventBatchError::Serialize)?;
            bytes.push(b'\n');
        }

        Ok(Self {
            day,
            bytes: bytes.into_boxed_slice(),
        })
    }

    #[must_use]
    pub(in crate::telemetry) const fn day(&self) -> UtcDay {
        self.day
    }

    #[must_use]
    pub(in crate::telemetry) const fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

impl fmt::Debug for EventBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventBatch")
            .field("day", &self.day)
            .field("byte_len", &self.byte_len())
            .finish()
    }
}

/// Failure to validate or serialize one append-only event batch.
#[derive(Debug)]
pub(in crate::telemetry) enum EventBatchError {
    Empty,
    RowDayMismatch {
        row_index: usize,
        batch_day: UtcDay,
        row_day: UtcDay,
    },
    Serialize(serde_json::Error),
}

impl fmt::Display for EventBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("telemetry event batch is empty"),
            Self::RowDayMismatch {
                row_index,
                batch_day,
                row_day,
            } => write!(
                formatter,
                "telemetry event row {row_index} belongs to {row_day}, not batch day {batch_day}"
            ),
            Self::Serialize(_) => formatter.write_str("failed to serialize telemetry event batch"),
        }
    }
}

impl Error for EventBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialize(error) => Some(error),
            Self::Empty | Self::RowDayMismatch { .. } => None,
        }
    }
}

/// Failure at one filesystem stage of an event-batch append.
#[derive(Debug)]
pub(in crate::telemetry) enum AppendEventBatchError {
    Prepare(io::Error),
    InspectTail(io::Error),
    RepairTail(io::Error),
    Write(io::Error),
}

impl fmt::Display for AppendEventBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepare(_) => formatter.write_str("failed to prepare telemetry event file"),
            Self::InspectTail(_) => {
                formatter.write_str("failed to inspect telemetry event file tail")
            }
            Self::RepairTail(_) => {
                formatter.write_str("failed to repair telemetry event file tail")
            }
            Self::Write(_) => formatter.write_str("failed to append telemetry event batch"),
        }
    }
}

impl Error for AppendEventBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Prepare(error)
            | Self::InspectTail(error)
            | Self::RepairTail(error)
            | Self::Write(error) => Some(error),
        }
    }
}

impl LockedStorage {
    /// Append one completely serialized batch to its canonical daily event file.
    ///
    /// Callers must already have rejected a day before the latest-opened-day
    /// high-water mark. This mechanism deliberately does not decide whether a
    /// day is closed.
    ///
    /// If an earlier failed append left a partial final line, this closes that
    /// line before writing the new batch. The daily allowance policy must count
    /// existing bytes, this possible repair byte, and EventBatch::byte_len.
    ///
    /// # Errors
    ///
    /// Returns a stage-specific error for file preparation, tail inspection,
    /// tail repair, or the batch write. A failed batch write may leave a partial
    /// final line; a later append preserves and closes it rather than overwriting
    /// inspectable data.
    pub(in crate::telemetry) fn append_event_batch(
        &mut self,
        batch: &EventBatch,
    ) -> Result<(), AppendEventBatchError> {
        let path = self.paths.event_file(batch.day());
        let mut file = prepare_event_file(&path).map_err(AppendEventBatchError::Prepare)?;
        append_to_handle(&mut file, &batch.bytes)
    }
}

fn prepare_event_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        // Protect new files immediately; set_permissions also repairs legacy files.
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;

    Ok(file)
}

fn append_to_handle(
    handle: &mut (impl io::Read + io::Write + io::Seek),
    bytes: &[u8],
) -> Result<(), AppendEventBatchError> {
    let needs_repair = tail_needs_repair(handle).map_err(AppendEventBatchError::InspectTail)?;
    if needs_repair {
        // A crash after this separate write only closes the previous malformed
        // line; it loses no new row and avoids copying the complete batch.
        handle
            .write_all(b"\n")
            .map_err(AppendEventBatchError::RepairTail)?;
    }

    // The production file uses append mode, so writes land at EOF regardless
    // of the seeks used to inspect its final byte.
    handle
        .write_all(bytes)
        .map_err(AppendEventBatchError::Write)
}

fn tail_needs_repair(handle: &mut (impl io::Read + io::Seek)) -> io::Result<bool> {
    let byte_len = handle.seek(SeekFrom::End(0))?;
    if byte_len == 0 {
        return Ok(false);
    }

    handle.seek(SeekFrom::End(-1))?;
    let mut tail = [0_u8; 1];
    handle.read_exact(&mut tail)?;
    Ok(tail[0] != b'\n')
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read, Seek, SeekFrom, Write},
    };

    use chrono::NaiveDate;

    use super::*;
    use crate::telemetry::schema::{DroppedOperation, StorageLimitV1};

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    fn row(day: UtcDay, operation: DroppedOperation) -> LowVolumeRow {
        LowVolumeRow::StorageLimit(StorageLimitV1::new(day, operation))
    }

    fn batch(day: UtcDay, operation: DroppedOperation) -> EventBatch {
        EventBatch::build(day, vec![row(day, operation)]).unwrap()
    }

    #[test]
    fn batch_preserves_row_order_and_terminates_each_json_line() {
        let day = day(3);
        let first = row(day, DroppedOperation::Use);
        let second = row(day, DroppedOperation::Command);
        let mut expected = serde_json::to_vec(&first).unwrap();
        expected.push(b'\n');
        expected.extend(serde_json::to_vec(&second).unwrap());
        expected.push(b'\n');

        let batch = EventBatch::build(day, vec![first, second]).unwrap();

        assert_eq!(batch.bytes.as_ref(), expected);
        assert_eq!(batch.byte_len(), expected.len());
        assert_eq!(batch.bytes.iter().filter(|byte| **byte == b'\n').count(), 2);
    }

    #[test]
    fn empty_batch_is_rejected() {
        assert!(matches!(
            EventBatch::build(day(3), Vec::new()),
            Err(EventBatchError::Empty)
        ));
    }

    #[test]
    fn row_from_another_day_reports_its_index() {
        let result = EventBatch::build(
            day(3),
            vec![
                row(day(3), DroppedOperation::Use),
                row(day(4), DroppedOperation::Command),
            ],
        );

        assert!(matches!(
            result,
            Err(EventBatchError::RowDayMismatch {
                row_index: 1,
                batch_day,
                row_day,
            }) if batch_day == day(3) && row_day == day(4)
        ));
    }

    #[test]
    fn invalid_batch_never_creates_an_event_file() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let path = storage.paths.event_file(day(3));

        let result = EventBatch::build(day(3), vec![row(day(4), DroppedOperation::Use)]);

        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn append_creates_the_canonical_event_file() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let batch = batch(day(3), DroppedOperation::Use);
        let path = storage.paths.event_file(day(3));

        storage.append_event_batch(&batch).unwrap();

        assert_eq!(fs::read(path).unwrap(), batch.bytes.as_ref());
    }

    #[test]
    fn later_batch_appends_without_changing_existing_bytes() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let first = batch(day(3), DroppedOperation::Use);
        let second = batch(day(3), DroppedOperation::Command);
        let path = storage.paths.event_file(day(3));
        storage.append_event_batch(&first).unwrap();
        let original = fs::read(&path).unwrap();

        storage.append_event_batch(&second).unwrap();

        let stored = fs::read(path).unwrap();
        assert_eq!(&stored[..original.len()], original);
        assert_eq!(&stored[original.len()..], second.bytes.as_ref());
    }

    #[test]
    fn append_closes_a_partial_final_line_before_the_batch() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let batch = batch(day(3), DroppedOperation::Use);
        let path = storage.paths.event_file(day(3));
        fs::write(&path, b"partial").unwrap();

        storage.append_event_batch(&batch).unwrap();

        let mut expected = b"partial".to_vec();
        expected.push(b'\n');
        expected.extend_from_slice(&batch.bytes);
        assert_eq!(fs::read(path).unwrap(), expected);
    }

    #[test]
    fn append_to_an_empty_file_adds_no_blank_line() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let batch = batch(day(3), DroppedOperation::Use);
        let path = storage.paths.event_file(day(3));
        fs::write(&path, []).unwrap();

        storage.append_event_batch(&batch).unwrap();

        assert_eq!(fs::read(path).unwrap(), batch.bytes.as_ref());
    }

    #[test]
    fn preparation_failure_is_typed() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let batch = batch(day(3), DroppedOperation::Use);
        fs::create_dir(storage.paths.event_file(day(3))).unwrap();

        let error = storage.append_event_batch(&batch).unwrap_err();

        assert!(matches!(error, AppendEventBatchError::Prepare(_)));
    }

    #[test]
    fn handle_failures_report_the_append_stage() {
        let mut inspect = FailingHandle::new(b"\n", Failure::Seek);
        assert!(matches!(
            append_to_handle(&mut inspect, b"\n"),
            Err(AppendEventBatchError::InspectTail(_))
        ));

        let mut repair = FailingHandle::new(b"partial", Failure::Write(1));
        assert!(matches!(
            append_to_handle(&mut repair, b"\n"),
            Err(AppendEventBatchError::RepairTail(_))
        ));

        let mut write = FailingHandle::new(b"\n", Failure::Write(1));
        assert!(matches!(
            append_to_handle(&mut write, b"\n"),
            Err(AppendEventBatchError::Write(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn event_files_are_owner_only_when_created_or_reopened() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let first = batch(day(3), DroppedOperation::Use);
        let first_path = storage.paths.event_file(day(3));
        storage.append_event_batch(&first).unwrap();
        assert_eq!(
            fs::metadata(first_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let second = batch(day(4), DroppedOperation::Command);
        let second_path = storage.paths.event_file(day(4));
        fs::write(&second_path, []).unwrap();
        fs::set_permissions(&second_path, fs::Permissions::from_mode(0o644)).unwrap();
        storage.append_event_batch(&second).unwrap();
        assert_eq!(
            fs::metadata(second_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[derive(Clone, Copy)]
    enum Failure {
        Seek,
        Write(usize),
    }

    struct FailingHandle {
        inner: Cursor<Vec<u8>>,
        failure: Failure,
        write_calls: usize,
    }

    impl FailingHandle {
        fn new(bytes: &[u8], failure: Failure) -> Self {
            Self {
                inner: Cursor::new(bytes.to_vec()),
                failure,
                write_calls: 0,
            }
        }
    }

    impl Read for FailingHandle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buffer)
        }
    }

    impl Seek for FailingHandle {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if matches!(self.failure, Failure::Seek) {
                return Err(io::Error::other("injected seek failure"));
            }
            self.inner.seek(position)
        }
    }

    impl Write for FailingHandle {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.write_calls += 1;
            if matches!(self.failure, Failure::Write(call) if call == self.write_calls) {
                return Err(io::Error::other("injected write failure"));
            }
            self.inner.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }
}
