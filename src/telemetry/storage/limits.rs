//! Daily storage accounting and low-volume event admission.
//!
//! Event selection measures both daily files and reserves the largest
//! `storage_limit` line the running build can emit. The decision itself is
//! pure and total; filesystem inspection and marker-preparation failures
//! suppress only the current event append so an aggregate snapshot can still
//! make its independent decision. A build that cannot prepare its marker
//! suppresses the attempt rather than appending against the fixed schema
//! bound. That keeps the exceptional path simple and leaves the day open.
//!
//! The telemetry lock covers both this module's bounded tail inspection and
//! [`super::events`] tail repair. Nothing can modify a telemetry file through
//! Symposium between those reads. Events publish before aggregate snapshots;
//! the later snapshot writer remeasures the enlarged event file and yields if
//! its own write would exceed the shared ceiling.

use std::{
    error::Error,
    fmt, fs,
    fs::File,
    io::{self, SeekFrom},
    str,
    sync::OnceLock,
};

use crate::telemetry::schema::{
    DroppedOperation, LowVolumeRow, MAX_STORAGE_LIMIT_LINE_BYTES, RowClassification,
    StorageLimitV1, TelemetryRow, UtcDay, VersionedRow as _, classify_row,
};

use super::{LockedStorage, events::EventBatch};

/// Shared daily ceiling for the event file, metric snapshot, and marker reserve.
pub(super) const DAILY_STORAGE_ALLOWANCE_BYTES: u64 = 8 * 1024 * 1024;

/// Filesystem sizes and final-line state used by the admission decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DailyUsage {
    event_bytes: u64,
    metric_bytes: u64,
    event_tail_needs_repair: bool,
    event_file_ends_with_marker: bool,
}

/// Low-volume event action selected under the daily storage ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub(in crate::telemetry) enum EventWrite {
    Append(EventBatch),
    Marker(EventBatch),
    Suppressed(EventSuppression),
}

impl EventWrite {
    /// Whether private state must remember that this day's event log is closed.
    #[must_use]
    pub(in crate::telemetry) const fn stops_event_recording(&self) -> bool {
        matches!(
            self,
            Self::Marker(_)
                | Self::Suppressed(
                    EventSuppression::AlreadyStopped | EventSuppression::MarkerDoesNotFit
                )
        )
    }
}

/// Reason no low-volume event bytes should be appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum EventSuppression {
    /// Private state or the final event-file line already closes this day.
    AlreadyStopped,
    /// Remaining shared allowance cannot hold even the marker.
    MarkerDoesNotFit,
    /// This build cannot construct a marker within the version 1 line bound.
    /// Only the current event append is suppressed.
    MarkerUnavailable,
    /// Existing daily usage could not be inspected safely.
    UsageUnavailable,
}

/// Failure while measuring one day's existing telemetry files.
#[derive(Debug)]
enum DailyUsageError {
    EventFile(io::Error),
    MetricFile(io::Error),
}

impl fmt::Display for DailyUsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventFile(_) => formatter.write_str("failed to inspect telemetry event usage"),
            Self::MetricFile(_) => formatter.write_str("failed to inspect telemetry metric usage"),
        }
    }
}

impl Error for DailyUsageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EventFile(error) | Self::MetricFile(error) => Some(error),
        }
    }
}

impl LockedStorage {
    /// Select the low-volume event write for one complete operation batch.
    ///
    /// A matching private stopped day avoids filesystem work. Otherwise the
    /// event file is also authoritative: a final marker repairs private state
    /// that was lost or reinitialized. Inspection or marker-preparation
    /// failure suppresses only this event append and does not close the day.
    pub(in crate::telemetry) fn plan_event_write(
        &self,
        batch: EventBatch,
        operation: DroppedOperation,
        stopped_day: Option<UtcDay>,
    ) -> EventWrite {
        let day = batch.day();
        if stopped_day == Some(day) {
            return EventWrite::Suppressed(EventSuppression::AlreadyStopped);
        }

        let usage = match self.inspect_daily_usage(day) {
            Ok(usage) => usage,
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "telemetry: daily usage unavailable; suppressing event append"
                );
                return EventWrite::Suppressed(EventSuppression::UsageUnavailable);
            }
        };

        let marker = match PreparedStorageLimit::new(day, operation) {
            Ok(marker) => Some(marker),
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "telemetry: storage-limit marker unavailable; suppressing event append"
                );
                None
            }
        };

        select_event_write(
            DAILY_STORAGE_ALLOWANCE_BYTES,
            usage,
            batch,
            marker,
            stopped_day,
        )
    }

    /// Measure both canonical files and inspect the event file's final line.
    ///
    /// # Errors
    ///
    /// Returns a typed I/O error when either file cannot be inspected. Missing
    /// files count as empty.
    fn inspect_daily_usage(&self, day: UtcDay) -> Result<DailyUsage, DailyUsageError> {
        let event = inspect_event_file(&self.paths.event_file(day), day)
            .map_err(DailyUsageError::EventFile)?;
        let metric_bytes =
            regular_file_len(&self.paths.metrics_file(day)).map_err(DailyUsageError::MetricFile)?;

        Ok(DailyUsage {
            event_bytes: event.byte_len,
            metric_bytes,
            event_tail_needs_repair: event.tail_needs_repair,
            event_file_ends_with_marker: event.ends_with_marker,
        })
    }
}

#[derive(Debug)]
struct PreparedStorageLimit {
    batch: EventBatch,
    reserved_line_bytes: u64,
}

impl PreparedStorageLimit {
    fn new(day: UtcDay, operation: DroppedOperation) -> Result<Self, MarkerPreparationError> {
        let batch = storage_limit_batch(day, operation)?;
        let reserved_line_bytes = marker_reservation(day, operation, batch.byte_len())?;

        Ok(Self {
            batch,
            reserved_line_bytes,
        })
    }
}

fn marker_reservation(
    day: UtcDay,
    operation: DroppedOperation,
    actual_line_bytes: usize,
) -> Result<u64, MarkerPreparationError> {
    // Every canonical UTC day and event id has a fixed wire length. The only
    // runtime-width field is the process-wide Symposium package version, so a
    // successful reservation is independent of later arguments in this run.
    static RESERVATION: OnceLock<u64> = OnceLock::new();

    if let Some(reservation) = RESERVATION.get() {
        return Ok(*reservation);
    }

    let mut reservation = saturating_u64(actual_line_bytes);
    for candidate in DroppedOperation::ALL {
        if candidate == operation {
            continue;
        }
        let candidate = storage_limit_batch(day, candidate)?;
        reservation = reservation.max(saturating_u64(candidate.byte_len()));
    }

    let maximum = saturating_u64(MAX_STORAGE_LIMIT_LINE_BYTES);
    if reservation > maximum {
        return Err(MarkerPreparationError::LineTooLong {
            required: reservation,
            maximum,
        });
    }

    Ok(*RESERVATION.get_or_init(|| reservation))
}

#[derive(Debug)]
enum MarkerPreparationError {
    Batch(super::events::EventBatchError),
    LineTooLong { required: u64, maximum: u64 },
}

impl fmt::Display for MarkerPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Batch(_) => formatter.write_str("failed to encode storage-limit marker"),
            Self::LineTooLong { required, maximum } => write!(
                formatter,
                "storage-limit marker needs {required} bytes, exceeding the {maximum}-byte v1 line bound"
            ),
        }
    }
}

impl Error for MarkerPreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Batch(error) => Some(error),
            Self::LineTooLong { .. } => None,
        }
    }
}

impl From<super::events::EventBatchError> for MarkerPreparationError {
    fn from(error: super::events::EventBatchError) -> Self {
        Self::Batch(error)
    }
}

fn storage_limit_batch(
    day: UtcDay,
    operation: DroppedOperation,
) -> Result<EventBatch, super::events::EventBatchError> {
    let row = StorageLimitV1::new(day, operation);
    EventBatch::build(day, vec![LowVolumeRow::StorageLimit(row)])
}

/// Make the all-or-marker decision without filesystem access.
fn select_event_write(
    allowance: u64,
    usage: DailyUsage,
    batch: EventBatch,
    marker: Option<PreparedStorageLimit>,
    stopped_day: Option<UtcDay>,
) -> EventWrite {
    if stopped_day == Some(batch.day()) || usage.event_file_ends_with_marker {
        return EventWrite::Suppressed(EventSuppression::AlreadyStopped);
    }

    let Some(marker) = marker else {
        return EventWrite::Suppressed(EventSuppression::MarkerUnavailable);
    };

    let used_bytes = usage.event_bytes.saturating_add(usage.metric_bytes);
    let repair_bytes = u64::from(usage.event_tail_needs_repair);
    let batch_bytes = saturating_u64(batch.byte_len());
    let append_bytes = used_bytes
        .saturating_add(repair_bytes)
        .saturating_add(batch_bytes)
        .saturating_add(marker.reserved_line_bytes);

    if append_bytes <= allowance {
        return EventWrite::Append(batch);
    }

    let marker_bytes = used_bytes
        .saturating_add(repair_bytes)
        .saturating_add(saturating_u64(marker.batch.byte_len()));
    if marker_bytes <= allowance {
        EventWrite::Marker(marker.batch)
    } else {
        EventWrite::Suppressed(EventSuppression::MarkerDoesNotFit)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EventFileUsage {
    byte_len: u64,
    tail_needs_repair: bool,
    ends_with_marker: bool,
}

fn inspect_event_file(path: &std::path::Path, day: UtcDay) -> io::Result<EventFileUsage> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(EventFileUsage::default());
        }
        Err(error) => return Err(error),
    };

    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "telemetry event path is not a regular file",
        ));
    }

    inspect_event_handle(&mut file, day)
}

fn inspect_event_handle(
    handle: &mut (impl io::Read + io::Seek),
    day: UtcDay,
) -> io::Result<EventFileUsage> {
    let byte_len = handle.seek(SeekFrom::End(0))?;
    if byte_len == 0 {
        return Ok(EventFileUsage::default());
    }

    // One additional byte proves either that the candidate starts at a line
    // boundary or that the final line is too long to be a version 1 marker.
    let inspection_limit = saturating_u64(MAX_STORAGE_LIMIT_LINE_BYTES).saturating_add(1);
    let read_len = byte_len.min(inspection_limit);
    let start = byte_len - read_len;
    handle.seek(SeekFrom::Start(start))?;

    let bounded_len = usize::try_from(read_len)
        .expect("BUG: the bounded storage-limit inspection must fit in usize");
    let mut tail = vec![0_u8; bounded_len];
    handle.read_exact(&mut tail)?;

    let tail_needs_repair = tail.last() != Some(&b'\n');
    let ends_with_marker = tail_is_storage_limit(&tail, start == 0, day);

    Ok(EventFileUsage {
        byte_len,
        tail_needs_repair,
        ends_with_marker,
    })
}

fn tail_is_storage_limit(tail: &[u8], starts_at_file_start: bool, day: UtcDay) -> bool {
    let line_end = tail
        .len()
        .saturating_sub(usize::from(tail.last() == Some(&b'\n')));
    let before_line = &tail[..line_end];
    let line_start = match before_line.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        None if starts_at_file_start => 0,
        None => return false,
    };
    let line = &before_line[line_start..];

    // The schema bound includes the line feed even when a failed write omitted
    // that final byte.
    if line.is_empty() || line.len().saturating_add(1) > MAX_STORAGE_LIMIT_LINE_BYTES {
        return false;
    }

    let Ok(line) = str::from_utf8(line) else {
        return false;
    };
    matches!(
        classify_row(line),
        RowClassification::Supported(TelemetryRow::LowVolume(
            LowVolumeRow::StorageLimit(row)
        )) if row.day() == day
    )
}

fn regular_file_len(path: &std::path::Path) -> io::Result<u64> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "telemetry metric path is not a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read, Seek, SeekFrom},
    };

    use chrono::NaiveDate;

    use super::*;

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    fn batch(day: UtcDay, operation: DroppedOperation) -> EventBatch {
        storage_limit_batch(day, operation).unwrap()
    }

    fn marker(day: UtcDay, operation: DroppedOperation) -> PreparedStorageLimit {
        PreparedStorageLimit::new(day, operation).unwrap()
    }

    fn usage(event_bytes: u64, metric_bytes: u64) -> DailyUsage {
        DailyUsage {
            event_bytes,
            metric_bytes,
            event_tail_needs_repair: false,
            event_file_ends_with_marker: false,
        }
    }

    #[test]
    fn exact_fit_appends_the_complete_batch_and_reserves_the_marker() {
        let day = day(3);
        let batch = batch(day, DroppedOperation::Use);
        let marker = marker(day, DroppedOperation::Configuration);
        let allowance = 17_u64
            .saturating_add(saturating_u64(batch.byte_len()))
            .saturating_add(marker.reserved_line_bytes);

        let decision =
            select_event_write(allowance, usage(10, 7), batch.clone(), Some(marker), None);

        assert_eq!(decision, EventWrite::Append(batch));
    }

    #[test]
    fn one_byte_over_replaces_the_whole_batch_with_a_marker() {
        let day = day(3);
        let batch = batch(day, DroppedOperation::Use);
        let marker = marker(day, DroppedOperation::Configuration);
        let allowance = saturating_u64(batch.byte_len())
            .saturating_add(marker.reserved_line_bytes)
            .saturating_sub(1);

        let decision = select_event_write(allowance, usage(0, 0), batch, Some(marker), None);

        assert!(matches!(decision, EventWrite::Marker(_)));
        assert!(decision.stops_event_recording());
    }

    #[test]
    fn multirow_batch_is_never_split_to_fit() {
        let day = day(3);
        let rows = vec![
            LowVolumeRow::StorageLimit(StorageLimitV1::new(day, DroppedOperation::Use)),
            LowVolumeRow::StorageLimit(StorageLimitV1::new(day, DroppedOperation::Command)),
        ];
        let batch = EventBatch::build(day, rows).unwrap();
        let prepared = marker(day, DroppedOperation::ManualSync);
        let allowance = saturating_u64(batch.byte_len())
            .saturating_add(prepared.reserved_line_bytes)
            .saturating_sub(1);

        let decision = select_event_write(allowance, usage(0, 0), batch, Some(prepared), None);

        assert!(matches!(decision, EventWrite::Marker(_)));
    }

    #[test]
    fn event_and_metric_bytes_both_consume_the_shared_allowance() {
        let day = day(3);
        let batch = batch(day, DroppedOperation::Use);
        let prepared = marker(day, DroppedOperation::Configuration);
        let allowance = 31_u64
            .saturating_add(saturating_u64(batch.byte_len()))
            .saturating_add(prepared.reserved_line_bytes);

        let exact = select_event_write(
            allowance,
            usage(11, 20),
            batch.clone(),
            Some(marker(day, DroppedOperation::Configuration)),
            None,
        );
        let over = select_event_write(allowance, usage(11, 21), batch, Some(prepared), None);

        assert!(matches!(exact, EventWrite::Append(_)));
        assert!(matches!(over, EventWrite::Marker(_)));
    }

    #[test]
    fn partial_tail_repair_consumes_one_byte() {
        let day = day(3);
        let batch = batch(day, DroppedOperation::Use);
        let prepared = marker(day, DroppedOperation::Configuration);
        let allowance = 10_u64
            .saturating_add(saturating_u64(batch.byte_len()))
            .saturating_add(prepared.reserved_line_bytes);
        let mut partial = usage(10, 0);
        partial.event_tail_needs_repair = true;

        let decision = select_event_write(allowance, partial, batch, Some(prepared), None);

        assert!(matches!(decision, EventWrite::Marker(_)));
    }

    #[test]
    fn no_room_for_the_marker_stops_the_day_without_a_write() {
        let day = day(3);
        let batch = batch(day, DroppedOperation::Use);
        let prepared = marker(day, DroppedOperation::Configuration);
        let allowance = prepared.reserved_line_bytes.saturating_sub(1);

        let decision = select_event_write(allowance, usage(0, 0), batch, Some(prepared), None);

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::MarkerDoesNotFit)
        );
        assert!(decision.stops_event_recording());
    }

    #[test]
    fn unavailable_marker_suppresses_only_the_current_attempt() {
        let day = day(3);
        let decision = select_event_write(
            u64::MAX,
            usage(0, 0),
            batch(day, DroppedOperation::Use),
            None,
            None,
        );

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::MarkerUnavailable)
        );
        assert!(!decision.stops_event_recording());
    }

    #[test]
    fn unavailable_marker_takes_precedence_over_exhausted_allowance() {
        let day = day(3);
        let decision = select_event_write(
            DAILY_STORAGE_ALLOWANCE_BYTES,
            usage(DAILY_STORAGE_ALLOWANCE_BYTES, 1),
            batch(day, DroppedOperation::Use),
            None,
            None,
        );

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::MarkerUnavailable)
        );
        assert!(!decision.stops_event_recording());
    }

    #[test]
    fn saturated_usage_cannot_wrap_into_an_append() {
        let day = day(3);
        let decision = select_event_write(
            DAILY_STORAGE_ALLOWANCE_BYTES,
            usage(u64::MAX, u64::MAX),
            batch(day, DroppedOperation::Use),
            Some(marker(day, DroppedOperation::Configuration)),
            None,
        );

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::MarkerDoesNotFit)
        );
    }

    #[test]
    fn stopped_day_matches_only_the_same_utc_day() {
        let stopped = day(3);
        let same_day = select_event_write(
            u64::MAX,
            usage(0, 0),
            batch(stopped, DroppedOperation::Use),
            Some(marker(stopped, DroppedOperation::Configuration)),
            Some(stopped),
        );
        let next_day = day(4);
        let later = select_event_write(
            u64::MAX,
            usage(0, 0),
            batch(next_day, DroppedOperation::Use),
            Some(marker(next_day, DroppedOperation::Configuration)),
            Some(stopped),
        );

        assert_eq!(
            same_day,
            EventWrite::Suppressed(EventSuppression::AlreadyStopped)
        );
        assert!(matches!(later, EventWrite::Append(_)));
    }

    #[test]
    fn every_operation_fits_the_running_builds_marker_reservation() {
        let day = day(3);
        let prepared = marker(day, DroppedOperation::Configuration);

        assert_eq!(DroppedOperation::ALL.len(), 7);
        for operation in DroppedOperation::ALL {
            let candidate = batch(day, operation);
            assert!(
                saturating_u64(candidate.byte_len()) <= prepared.reserved_line_bytes,
                "{operation:?} marker exceeded the reservation"
            );
        }
        assert!(
            prepared.reserved_line_bytes <= saturating_u64(MAX_STORAGE_LIMIT_LINE_BYTES) / 2,
            "the running build no longer has comfortable marker-line headroom"
        );
    }

    #[test]
    fn marker_with_exactly_its_reserved_space_lands_on_the_allowance() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let day = day(3);
        let path = storage.paths.event_file(day);
        let mut existing = vec![b'x'; 64];
        *existing.last_mut().unwrap() = b'\n';
        fs::write(&path, &existing).unwrap();

        let usage = storage.inspect_daily_usage(day).unwrap();
        let prepared = marker(day, DroppedOperation::Configuration);
        assert_eq!(
            saturating_u64(prepared.batch.byte_len()),
            prepared.reserved_line_bytes
        );
        let allowance = usage
            .event_bytes
            .saturating_add(prepared.reserved_line_bytes);
        let decision = select_event_write(
            allowance,
            usage,
            batch(day, DroppedOperation::Use),
            Some(prepared),
            None,
        );
        let EventWrite::Marker(marker) = decision else {
            panic!("exact marker space did not select the marker")
        };

        storage.append_event_batch(&marker).unwrap();

        assert_eq!(fs::metadata(path).unwrap().len(), allowance);
    }

    #[test]
    fn missing_daily_files_have_zero_usage() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();

        let usage = storage.inspect_daily_usage(day(3)).unwrap();

        assert_eq!(usage, DailyUsage::default());
    }

    #[test]
    fn daily_usage_measures_both_files_and_the_repair_byte() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let day = day(3);
        fs::write(storage.paths.event_file(day), b"partial").unwrap();
        fs::write(storage.paths.metrics_file(day), b"metrics").unwrap();

        let usage = storage.inspect_daily_usage(day).unwrap();

        assert_eq!(usage.event_bytes, 7);
        assert_eq!(usage.metric_bytes, 7);
        assert!(usage.event_tail_needs_repair);
        assert!(!usage.event_file_ends_with_marker);
    }

    #[test]
    fn final_marker_recovers_a_missing_private_stopped_day() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let day = day(3);
        let marker = batch(day, DroppedOperation::Configuration);
        storage.append_event_batch(&marker).unwrap();
        let original = fs::read(storage.paths.event_file(day)).unwrap();

        let decision = storage.plan_event_write(
            batch(day, DroppedOperation::Use),
            DroppedOperation::Use,
            None,
        );

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::AlreadyStopped)
        );
        assert!(decision.stops_event_recording());
        assert_eq!(fs::read(storage.paths.event_file(day)).unwrap(), original);
    }

    #[test]
    fn complete_unterminated_marker_still_closes_the_day() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let day = day(3);
        let row = StorageLimitV1::new(day, DroppedOperation::Configuration);
        fs::write(
            storage.paths.event_file(day),
            serde_json::to_vec(&row).unwrap(),
        )
        .unwrap();

        let usage = storage.inspect_daily_usage(day).unwrap();

        assert!(usage.event_tail_needs_repair);
        assert!(usage.event_file_ends_with_marker);
    }

    #[test]
    fn marker_for_another_day_does_not_close_this_file() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let file_day = day(3);
        let row = StorageLimitV1::new(day(4), DroppedOperation::Configuration);
        let mut line = serde_json::to_vec(&row).unwrap();
        line.push(b'\n');
        fs::write(storage.paths.event_file(file_day), line).unwrap();

        let usage = storage.inspect_daily_usage(file_day).unwrap();

        assert!(!usage.event_file_ends_with_marker);
    }

    #[test]
    fn long_final_non_marker_line_is_inspected_with_a_bounded_read() {
        let bytes = vec![b'x'; MAX_STORAGE_LIMIT_LINE_BYTES * 4];
        let mut handle = CountingHandle::new(bytes);

        let usage = inspect_event_handle(&mut handle, day(3)).unwrap();

        assert!(!usage.ends_with_marker);
        assert!(usage.tail_needs_repair);
        assert!(handle.bytes_read <= MAX_STORAGE_LIMIT_LINE_BYTES + 1);
    }

    #[test]
    fn usage_inspection_failure_suppresses_only_this_event_attempt() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = LockedStorage::try_acquire(temporary.path()).unwrap();
        let day = day(3);
        fs::create_dir(storage.paths.event_file(day)).unwrap();

        let decision = storage.plan_event_write(
            batch(day, DroppedOperation::Use),
            DroppedOperation::Use,
            None,
        );

        assert_eq!(
            decision,
            EventWrite::Suppressed(EventSuppression::UsageUnavailable)
        );
        assert!(!decision.stops_event_recording());
    }

    struct CountingHandle {
        inner: Cursor<Vec<u8>>,
        bytes_read: usize,
    }

    impl CountingHandle {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                inner: Cursor::new(bytes),
                bytes_read: 0,
            }
        }
    }

    impl Read for CountingHandle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(buffer)?;
            self.bytes_read = self.bytes_read.saturating_add(read);
            Ok(read)
        }
    }

    impl Seek for CountingHandle {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }
}
