use std::{fs, io, time::Duration};

use chrono::{NaiveDate, TimeZone as _, Utc};

use super::*;
use crate::telemetry::{
    schema::{
        DroppedOperation, HookAgent, HookOutcome, HookSurface, LowVolumeRow, RowClassification,
        StorageLimitV1, TelemetryRow, UtcDay, UtcSecond, classify_row, recorded_data_example_row,
    },
    state::{
        HookInvocationMetricObservation, IDENTIFIER_WINDOW_TEST_STATE, StagedRecording,
        TelemetryStateV1,
    },
    storage::{
        Storage, TestWriteOperation, TestWrites,
        limits::{DAILY_STORAGE_ALLOWANCE_BYTES, reserved_storage_limit_line_bytes},
        maintenance::MaintenanceOutcome,
        metrics::MetricSnapshot,
        writes::{FsWrites, RecordingWriteAccess, RecordingWrites},
    },
};

#[derive(Debug, Default)]
struct PanickingEventWrites;

impl RecordingWrites for PanickingEventWrites {
    fn replace_private_state(
        &mut self,
        access: RecordingWriteAccess<'_>,
        bytes: &[u8],
    ) -> Result<(), AtomicReplaceError> {
        FsWrites.replace_private_state(access, bytes)
    }

    fn append_event_batch(
        &mut self,
        _access: RecordingWriteAccess<'_>,
        _batch: &EventBatch,
    ) -> Result<(), AppendEventBatchError> {
        panic!("injected publication panic")
    }

    fn replace_metric_snapshot(
        &mut self,
        access: RecordingWriteAccess<'_>,
        snapshot: &PreparedMetricSnapshot,
    ) -> Result<(), AtomicReplaceError> {
        FsWrites.replace_metric_snapshot(access, snapshot)
    }
}

fn day(day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
}

fn completed_at() -> UtcSecond {
    UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 3, 10, 2, 11).unwrap())
}

fn invocation() -> HookInvocationMetricObservation<'static> {
    HookInvocationMetricObservation {
        agent: HookAgent::Claude,
        hook: HookSurface::UserPromptSubmit,
        outcome: HookOutcome::Ok,
        duration: Duration::from_millis(7),
        vendor_session_id: None,
        plugin_attempts: Vec::new(),
        extension: None,
    }
}

fn stage(state: TelemetryStateV1) -> StagedRecording {
    state
        .stage_hook_invocation(completed_at(), MetricSnapshot::empty(day(3)), invocation())
        .unwrap()
}

fn staged_recording() -> StagedRecording {
    stage(TelemetryStateV1::decode_for_test(
        IDENTIFIER_WINDOW_TEST_STATE,
    ))
}

fn staged_recording_with_maintenance() -> (StagedRecording, MaintenanceDue) {
    let mut state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
    let accepted = state.observe_recording(completed_at()).unwrap();
    let due = state.schedule_maintenance(&accepted).unwrap();
    (stage(state), due)
}

fn stopped_staged_recording() -> StagedRecording {
    let source = IDENTIFIER_WINDOW_TEST_STATE.replacen(
        "version = 1\n",
        "version = 1\nstorage-limit-day = \"2026-08-03\"\n",
        1,
    );
    stage(TelemetryStateV1::decode_for_test(&source))
}

fn command_batch() -> EventBatch {
    let RowClassification::Supported(TelemetryRow::LowVolume(row)) =
        classify_row(recorded_data_example_row("command"))
    else {
        panic!("recorded command example must be a supported low-volume row");
    };
    EventBatch::build(day(3), vec![row]).unwrap()
}

fn event_request() -> EventPublicationRequest {
    EventPublicationRequest::new(command_batch(), DroppedOperation::Command)
}

fn marker_batch(operation: DroppedOperation) -> EventBatch {
    let row = LowVolumeRow::StorageLimit(StorageLimitV1::new(day(3), operation));
    EventBatch::build(day(3), vec![row]).unwrap()
}

fn storage_with_writer(
    temporary: &tempfile::TempDir,
    writes: TestWrites,
) -> LockedStorage<TestWrites> {
    LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap()
}

#[test]
fn private_state_persists_before_events_and_metrics_publish() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    assert!(observed.calls().is_empty());
    let persisted = storage.persist_recording(prepared).unwrap();
    assert_eq!(observed.calls(), [TestWriteOperation::PrivateState]);

    let outcome = persisted.publish().0;

    assert_eq!(outcome.event, EventPublication::Published);
    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        observed.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::EventBatch,
            TestWriteOperation::MetricSnapshot,
        ]
    );
}

#[test]
fn maintenance_is_released_only_after_persistence_and_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage = Storage::try_acquire(temporary.path()).unwrap();
    let state_path = storage.paths.state_file().to_path_buf();
    let (staged, due) = staged_recording_with_maintenance();
    let prepared = storage.prepare_recording(staged, None, Some(due)).unwrap();

    let persisted = storage.persist_recording(prepared).unwrap();
    let mut persisted_state = TelemetryStateV1::decode_for_test(
        std::str::from_utf8(&fs::read(state_path).unwrap()).unwrap(),
    );
    let accepted = persisted_state.observe_recording(completed_at()).unwrap();
    assert!(persisted_state.schedule_maintenance(&accepted).is_none());

    let (outcome, released) = persisted.publish();
    let maintenance = storage.maintain(released.unwrap());

    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        maintenance,
        MaintenanceOutcome::Completed {
            removed_daily_files: 0,
            removed_temporary_files: 0,
            inspection_failures: 0,
            removal_failures: 0,
        }
    );
    assert!(storage.paths.lock_file().is_file());
}

#[test]
fn publication_failure_still_releases_scheduled_maintenance() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing_on([
        TestWriteOperation::EventBatch,
        TestWriteOperation::MetricSnapshot,
    ]);
    let mut storage = storage_with_writer(&temporary, writes);
    let (staged, due) = staged_recording_with_maintenance();
    let due_day = due.day();
    let prepared = storage
        .prepare_recording(staged, Some(event_request()), Some(due))
        .unwrap();

    let (outcome, released) = storage.persist_recording(prepared).unwrap().publish();

    assert!(matches!(outcome.event, EventPublication::Failed(_)));
    assert!(matches!(outcome.metric, MetricPublication::Failed(_)));
    assert_eq!(released.unwrap().day(), due_day);
}

#[test]
fn private_state_failure_mints_no_receipt_and_touches_no_public_file() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::PrivateState);
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let event_path = storage.paths.event_file(day(3));
    let metric_path = storage.paths.metrics_file(day(3));
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    let Err(error) = storage.persist_recording(prepared) else {
        panic!("private-state failure unexpectedly minted a receipt");
    };

    assert_eq!(error, PersistRecordingError::Write(io::ErrorKind::Other));
    assert_eq!(observed.calls(), [TestWriteOperation::PrivateState]);
    assert!(!event_path.exists());
    assert!(!metric_path.exists());
}

#[test]
fn event_failure_does_not_suppress_metric_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::EventBatch);
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(
        outcome.event,
        EventPublication::Failed(EventPublicationFailure::Write(io::ErrorKind::Other))
    );
    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        observed.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::EventBatch,
            TestWriteOperation::MetricSnapshot,
        ]
    );
}

#[test]
fn metric_failure_does_not_erase_successful_event_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::MetricSnapshot);
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(outcome.event, EventPublication::Published);
    assert_eq!(
        outcome.metric,
        MetricPublication::Failed(MetricPublicationFailure::Write(io::ErrorKind::Other))
    );
}

#[test]
fn both_publication_failures_are_reported_independently() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing_on([
        TestWriteOperation::EventBatch,
        TestWriteOperation::MetricSnapshot,
    ]);
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(
        outcome,
        PublishOutcome {
            event: EventPublication::Failed(EventPublicationFailure::Write(io::ErrorKind::Other)),
            metric: MetricPublication::Failed(MetricPublicationFailure::Write(
                io::ErrorKind::Other
            )),
        }
    );
}

#[test]
fn an_aggregate_only_recording_has_no_event_write() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(staged_recording(), None, None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(outcome.event, EventPublication::NotRequested);
    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        observed.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::MetricSnapshot,
        ]
    );
}

#[test]
fn an_already_stopped_event_day_still_publishes_metrics() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let prepared = storage
        .prepare_recording(stopped_staged_recording(), Some(event_request()), None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(
        outcome.event,
        EventPublication::Suppressed(EventSuppression::AlreadyStopped)
    );
    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        observed.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::MetricSnapshot,
        ]
    );
}

#[test]
fn over_ceiling_metric_is_suppressed_after_private_state_persists() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let event_path = storage.paths.event_file(day(3));
    let mut full_event_file = vec![b'x'; usize::try_from(DAILY_STORAGE_ALLOWANCE_BYTES).unwrap()];
    *full_event_file.last_mut().unwrap() = b'\n';
    fs::write(event_path, full_event_file).unwrap();
    let prepared = storage
        .prepare_recording(staged_recording(), None, None)
        .unwrap();

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(outcome.event, EventPublication::NotRequested);
    assert_eq!(
        outcome.metric,
        MetricPublication::Suppressed(MetricSuppression::DailyAllowanceExceeded)
    );
    assert_eq!(observed.calls(), [TestWriteOperation::PrivateState]);
}

#[test]
fn failed_marker_append_leaves_the_persisted_day_stopped() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::EventBatch);
    let mut storage = storage_with_writer(&temporary, writes);
    let marker_bytes = reserved_storage_limit_line_bytes(day(3));
    let existing_bytes = DAILY_STORAGE_ALLOWANCE_BYTES - marker_bytes;
    let mut existing = vec![b'x'; usize::try_from(existing_bytes).unwrap()];
    *existing.last_mut().unwrap() = b'\n';
    fs::write(storage.paths.event_file(day(3)), existing).unwrap();
    let request = event_request();
    assert!(u64::try_from(request.batch.byte_len()).unwrap() > marker_bytes);
    let prepared = storage
        .prepare_recording(staged_recording(), Some(request), None)
        .unwrap();
    assert!(matches!(prepared.event, Some(EventWrite::Marker(_))));

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;
    let state = storage.load_state().unwrap().unwrap();

    assert!(matches!(outcome.event, EventPublication::Failed(_)));
    assert_eq!(state.storage_limit_day(), Some(day(3)));
}

#[test]
fn marker_that_does_not_fit_stops_the_day_without_touching_the_event_file() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let event_path = storage.paths.event_file(day(3));
    let mut existing = vec![b'x'; usize::try_from(DAILY_STORAGE_ALLOWANCE_BYTES).unwrap()];
    *existing.last_mut().unwrap() = b'\n';
    fs::write(&event_path, &existing).unwrap();
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();
    assert_eq!(
        prepared.event,
        Some(EventWrite::Suppressed(EventSuppression::MarkerDoesNotFit))
    );

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;
    let state = storage.load_state().unwrap().unwrap();

    assert_eq!(
        outcome.event,
        EventPublication::Suppressed(EventSuppression::MarkerDoesNotFit)
    );
    assert_eq!(state.storage_limit_day(), Some(day(3)));
    assert_eq!(fs::read(event_path).unwrap(), existing);
    assert_eq!(observed.calls(), [TestWriteOperation::PrivateState]);
}

#[test]
fn a_successful_marker_releases_its_reservation_for_metric_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let mut storage = storage_with_writer(&temporary, writes);
    let operation = DroppedOperation::Configuration;
    let marker_bytes = u64::try_from(marker_batch(operation).byte_len()).unwrap();
    let existing_bytes = DAILY_STORAGE_ALLOWANCE_BYTES - marker_bytes;
    let mut existing = vec![b'x'; usize::try_from(existing_bytes).unwrap()];
    *existing.last_mut().unwrap() = b'\n';
    let event_path = storage.paths.event_file(day(3));
    fs::write(&event_path, existing).unwrap();
    let state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
    let staged = StagedRecording::from_parts_for_test(state, MetricSnapshot::empty(day(3)));
    let request = EventPublicationRequest::new(command_batch(), operation);
    let prepared = storage
        .prepare_recording(staged, Some(request), None)
        .unwrap();
    assert!(matches!(prepared.event, Some(EventWrite::Marker(_))));
    assert_eq!(prepared.metric.byte_len(), 0);
    assert!(
        existing_bytes
            .saturating_add(marker_bytes)
            .saturating_add(reserved_storage_limit_line_bytes(day(3)))
            > DAILY_STORAGE_ALLOWANCE_BYTES
    );

    let outcome = storage.persist_recording(prepared).unwrap().publish().0;

    assert_eq!(outcome.event, EventPublication::Published);
    assert_eq!(outcome.metric, MetricPublication::Published);
    assert_eq!(
        fs::metadata(event_path).unwrap().len(),
        DAILY_STORAGE_ALLOWANCE_BYTES
    );
    assert_eq!(
        observed.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::EventBatch,
            TestWriteOperation::MetricSnapshot,
        ]
    );
}

#[test]
fn mismatched_event_day_is_rejected_before_persistence() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let storage = storage_with_writer(&temporary, writes);
    let row = LowVolumeRow::StorageLimit(StorageLimitV1::new(day(4), DroppedOperation::Command));
    let request = EventPublicationRequest::new(
        EventBatch::build(day(4), vec![row]).unwrap(),
        DroppedOperation::Command,
    );

    let error = storage
        .prepare_recording(staged_recording(), Some(request), None)
        .unwrap_err();

    assert!(matches!(
        error,
        PrepareRecordingError::EventDayMismatch {
            state_day,
            event_day
        } if state_day == day(3) && event_day == day(4)
    ));
    assert!(observed.calls().is_empty());
}

#[test]
fn mismatched_metric_day_is_rejected_before_persistence() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let observed = writes.clone();
    let storage = storage_with_writer(&temporary, writes);
    let state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
    let staged = StagedRecording::from_parts_for_test(state, MetricSnapshot::empty(day(4)));

    let error = storage.prepare_recording(staged, None, None).unwrap_err();

    assert!(matches!(
        error,
        PrepareRecordingError::MetricDayMismatch {
            state_day,
            metric_day
        } if state_day == day(3) && metric_day == day(4)
    ));
    assert!(observed.calls().is_empty());
}

#[test]
#[should_panic(expected = "persisted telemetry recording dropped before publication")]
fn dropping_a_persisted_receipt_without_publication_is_loud() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage = Storage::try_acquire(temporary.path()).unwrap();
    let prepared = storage
        .prepare_recording(staged_recording(), None, None)
        .unwrap();

    drop(storage.persist_recording(prepared).unwrap());
}

#[test]
#[should_panic(expected = "injected publication panic")]
fn publication_panic_is_not_misreported_as_a_dropped_receipt() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage =
        LockedStorage::try_acquire_with_writer(temporary.path(), PanickingEventWrites).unwrap();
    let prepared = storage
        .prepare_recording(staged_recording(), Some(event_request()), None)
        .unwrap();

    let _ = storage.persist_recording(prepared).unwrap().publish();
}

#[test]
#[should_panic(expected = "injected pre-publication panic")]
fn unwind_before_publication_does_not_double_panic_in_drop() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage = Storage::try_acquire(temporary.path()).unwrap();
    let prepared = storage
        .prepare_recording(staged_recording(), None, None)
        .unwrap();
    let _persisted = storage.persist_recording(prepared).unwrap();

    panic!("injected pre-publication panic");
}

#[test]
fn event_failures_retain_only_the_stage_and_error_kind() {
    let cases = [
        (
            AppendEventBatchError::Prepare(io::Error::from(io::ErrorKind::PermissionDenied)),
            EventPublicationFailure::Prepare(io::ErrorKind::PermissionDenied),
        ),
        (
            AppendEventBatchError::InspectTail(io::Error::from(io::ErrorKind::InvalidData)),
            EventPublicationFailure::InspectTail(io::ErrorKind::InvalidData),
        ),
        (
            AppendEventBatchError::RepairTail(io::Error::from(io::ErrorKind::WriteZero)),
            EventPublicationFailure::RepairTail(io::ErrorKind::WriteZero),
        ),
        (
            AppendEventBatchError::Write(io::Error::from(io::ErrorKind::OutOfMemory)),
            EventPublicationFailure::Write(io::ErrorKind::OutOfMemory),
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(EventPublicationFailure::from(error), expected);
    }
}

#[test]
fn atomic_failures_retain_only_the_stage_and_error_kind() {
    let persist_cases = [
        (
            AtomicReplaceError::Create(io::Error::from(io::ErrorKind::PermissionDenied)),
            PersistRecordingError::Create(io::ErrorKind::PermissionDenied),
        ),
        (
            AtomicReplaceError::Write(io::Error::from(io::ErrorKind::WriteZero)),
            PersistRecordingError::Write(io::ErrorKind::WriteZero),
        ),
        (
            AtomicReplaceError::Persist(io::Error::from(io::ErrorKind::AlreadyExists)),
            PersistRecordingError::Persist(io::ErrorKind::AlreadyExists),
        ),
    ];

    for (error, expected) in persist_cases {
        assert_eq!(PersistRecordingError::from(error), expected);
    }

    let metric_cases = [
        (
            AtomicReplaceError::Create(io::Error::from(io::ErrorKind::PermissionDenied)),
            MetricPublicationFailure::Create(io::ErrorKind::PermissionDenied),
        ),
        (
            AtomicReplaceError::Write(io::Error::from(io::ErrorKind::WriteZero)),
            MetricPublicationFailure::Write(io::ErrorKind::WriteZero),
        ),
        (
            AtomicReplaceError::Persist(io::Error::from(io::ErrorKind::AlreadyExists)),
            MetricPublicationFailure::Persist(io::ErrorKind::AlreadyExists),
        ),
    ];

    for (error, expected) in metric_cases {
        assert_eq!(MetricPublicationFailure::from(error), expected);
    }
}
