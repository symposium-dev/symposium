//! Complete filesystem writes performed while telemetry storage is locked.
//!
//! This storage-private adapter accepts only fully prepared private state,
//! event batches, and metric snapshots. The byte-handle seam in `events`
//! remains private to that module and exists only to distinguish its append
//! failure stages. The recording transaction owns the ordering of these
//! operations.
//!
//! [`RecordingWriteAccess`] carries the canonical paths from the locked
//! storage value, so callers cannot supply a different destination.

use super::{
    atomic::{self, AtomicReplaceError},
    events::{self, AppendEventBatchError, EventBatch},
    metrics::{self, PreparedMetricSnapshot},
    paths::StoragePaths,
};

/// Storage authority supplied only by a locked storage value.
#[derive(Clone, Copy)]
pub(super) struct RecordingWriteAccess<'a> {
    paths: &'a StoragePaths,
}

impl<'a> RecordingWriteAccess<'a> {
    pub(super) const fn new(paths: &'a StoragePaths) -> Self {
        Self { paths }
    }
}

/// Storage-private adapter over complete filesystem operations.
pub(super) trait RecordingWrites {
    /// Replace already-encoded private state.
    fn replace_private_state(
        &mut self,
        access: RecordingWriteAccess<'_>,
        bytes: &[u8],
    ) -> Result<(), AtomicReplaceError>;

    /// Append one complete event batch.
    fn append_event_batch(
        &mut self,
        access: RecordingWriteAccess<'_>,
        batch: &EventBatch,
    ) -> Result<(), AppendEventBatchError>;

    /// Replace one complete metric snapshot.
    fn replace_metric_snapshot(
        &mut self,
        access: RecordingWriteAccess<'_>,
        snapshot: &PreparedMetricSnapshot,
    ) -> Result<(), AtomicReplaceError>;
}

/// Production adapter for complete filesystem writes.
#[derive(Debug, Default)]
pub(super) struct FsWrites;

impl RecordingWrites for FsWrites {
    fn replace_private_state(
        &mut self,
        access: RecordingWriteAccess<'_>,
        bytes: &[u8],
    ) -> Result<(), AtomicReplaceError> {
        atomic::replace(access.paths.state_file(), bytes)
    }

    fn append_event_batch(
        &mut self,
        access: RecordingWriteAccess<'_>,
        batch: &EventBatch,
    ) -> Result<(), AppendEventBatchError> {
        events::append_event_batch(&access.paths.event_file(batch.day()), batch)
    }

    fn replace_metric_snapshot(
        &mut self,
        access: RecordingWriteAccess<'_>,
        snapshot: &PreparedMetricSnapshot,
    ) -> Result<(), AtomicReplaceError> {
        metrics::replace_metric_snapshot(&access.paths.metrics_file(snapshot.day()), snapshot)
    }
}

#[cfg(test)]
mod test_support {
    use std::{cell::RefCell, collections::BTreeSet, io, rc::Rc};

    use super::*;

    /// One whole operation observed by the test writer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(in crate::telemetry::storage) enum TestWriteOperation {
        PrivateState,
        EventBatch,
        MetricSnapshot,
    }

    #[derive(Debug, Default)]
    struct TestWriteState {
        calls: Vec<TestWriteOperation>,
        failures: BTreeSet<TestWriteOperation>,
    }

    /// Delegating writer with operation-level, one-shot injected failures.
    #[derive(Debug, Clone, Default)]
    pub(in crate::telemetry::storage) struct TestWrites {
        state: Rc<RefCell<TestWriteState>>,
    }

    impl TestWrites {
        #[must_use]
        pub(in crate::telemetry::storage) fn failing(operation: TestWriteOperation) -> Self {
            Self::failing_on([operation])
        }

        #[must_use]
        pub(in crate::telemetry::storage) fn failing_on(
            operations: impl IntoIterator<Item = TestWriteOperation>,
        ) -> Self {
            Self {
                state: Rc::new(RefCell::new(TestWriteState {
                    calls: Vec::new(),
                    failures: operations.into_iter().collect(),
                })),
            }
        }

        #[must_use]
        pub(in crate::telemetry::storage) fn calls(&self) -> Vec<TestWriteOperation> {
            self.state.borrow().calls.clone()
        }

        fn record(&self, operation: TestWriteOperation) -> bool {
            let mut state = self.state.borrow_mut();
            state.calls.push(operation);
            state.failures.remove(&operation)
        }
    }

    impl RecordingWrites for TestWrites {
        fn replace_private_state(
            &mut self,
            access: RecordingWriteAccess<'_>,
            bytes: &[u8],
        ) -> Result<(), AtomicReplaceError> {
            if self.record(TestWriteOperation::PrivateState) {
                return Err(injected_atomic_failure());
            }
            FsWrites.replace_private_state(access, bytes)
        }

        fn append_event_batch(
            &mut self,
            access: RecordingWriteAccess<'_>,
            batch: &EventBatch,
        ) -> Result<(), AppendEventBatchError> {
            if self.record(TestWriteOperation::EventBatch) {
                return Err(AppendEventBatchError::Write(injected_io_failure()));
            }
            FsWrites.append_event_batch(access, batch)
        }

        fn replace_metric_snapshot(
            &mut self,
            access: RecordingWriteAccess<'_>,
            snapshot: &PreparedMetricSnapshot,
        ) -> Result<(), AtomicReplaceError> {
            if self.record(TestWriteOperation::MetricSnapshot) {
                return Err(injected_atomic_failure());
            }
            FsWrites.replace_metric_snapshot(access, snapshot)
        }
    }

    fn injected_atomic_failure() -> AtomicReplaceError {
        AtomicReplaceError::Write(injected_io_failure())
    }

    fn injected_io_failure() -> io::Error {
        io::Error::other("injected whole-operation write failure")
    }
}

#[cfg(test)]
pub(super) use test_support::{TestWriteOperation, TestWrites};

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::NaiveDate;

    use super::*;
    use crate::telemetry::{
        schema::{DroppedOperation, LowVolumeRow, StorageLimitV1, UtcDay},
        state::{IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1},
        storage::{LockedStorage, metrics::MetricSnapshot, private_state::ReplaceStateError},
    };

    fn day() -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap())
    }

    fn state() -> TelemetryStateV1 {
        TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE)
    }

    fn event_batch() -> EventBatch {
        let day = day();
        EventBatch::build(
            day,
            vec![LowVolumeRow::StorageLimit(StorageLimitV1::new(
                day,
                DroppedOperation::Use,
            ))],
        )
        .unwrap()
    }

    fn metric_snapshot() -> PreparedMetricSnapshot {
        MetricSnapshot::empty(day()).prepare().unwrap()
    }

    #[test]
    fn complete_operations_route_through_the_writer_in_call_order() {
        let temporary = tempfile::tempdir().unwrap();
        let writes = TestWrites::default();
        let observed = writes.clone();
        let mut storage = LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap();
        let state = state();
        let events = event_batch();
        let metrics = metric_snapshot();

        storage.replace_state(&state).unwrap();
        storage.append_event_batch(&events).unwrap();
        storage.replace_metric_snapshot(&metrics).unwrap();

        assert_eq!(
            observed.calls(),
            [
                TestWriteOperation::PrivateState,
                TestWriteOperation::EventBatch,
                TestWriteOperation::MetricSnapshot,
            ]
        );
        assert!(storage.paths.state_file().is_file());
        assert!(storage.paths.event_file(day()).is_file());
        assert!(storage.paths.metrics_file(day()).is_file());
    }

    #[test]
    fn private_state_failure_preserves_the_existing_file() {
        let temporary = tempfile::tempdir().unwrap();
        let writes = TestWrites::failing(TestWriteOperation::PrivateState);
        let mut storage = LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap();
        fs::write(storage.paths.state_file(), b"old state").unwrap();

        let error = storage.replace_state(&state()).unwrap_err();

        assert!(matches!(error, ReplaceStateError::Replace(_)));
        assert_eq!(fs::read(storage.paths.state_file()).unwrap(), b"old state");
    }

    #[test]
    fn event_failure_preserves_the_existing_file() {
        let temporary = tempfile::tempdir().unwrap();
        let writes = TestWrites::failing(TestWriteOperation::EventBatch);
        let mut storage = LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap();
        let path = storage.paths.event_file(day());
        fs::write(&path, b"old event\n").unwrap();

        let error = storage.append_event_batch(&event_batch()).unwrap_err();

        assert!(matches!(error, AppendEventBatchError::Write(_)));
        assert_eq!(fs::read(path).unwrap(), b"old event\n");
    }

    #[test]
    fn metric_failure_preserves_the_existing_file() {
        let temporary = tempfile::tempdir().unwrap();
        let writes = TestWrites::failing(TestWriteOperation::MetricSnapshot);
        let mut storage = LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap();
        let path = storage.paths.metrics_file(day());
        fs::write(&path, b"old metric\n").unwrap();

        let error = storage
            .replace_metric_snapshot(&metric_snapshot())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "failed to replace aggregate metric snapshot"
        );
        assert_eq!(fs::read(path).unwrap(), b"old metric\n");
    }

    #[test]
    fn multiple_failures_fire_independently_and_only_once() {
        let temporary = tempfile::tempdir().unwrap();
        let writes = TestWrites::failing_on([
            TestWriteOperation::EventBatch,
            TestWriteOperation::MetricSnapshot,
        ]);
        let observed = writes.clone();
        let mut storage = LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap();
        let events = event_batch();
        let metrics = metric_snapshot();

        assert!(storage.append_event_batch(&events).is_err());
        assert!(storage.replace_metric_snapshot(&metrics).is_err());
        storage.append_event_batch(&events).unwrap();
        storage.replace_metric_snapshot(&metrics).unwrap();

        assert_eq!(
            observed.calls(),
            [
                TestWriteOperation::EventBatch,
                TestWriteOperation::MetricSnapshot,
                TestWriteOperation::EventBatch,
                TestWriteOperation::MetricSnapshot,
            ]
        );
    }
}
