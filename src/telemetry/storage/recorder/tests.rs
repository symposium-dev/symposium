use std::{fs, io, time::Duration};

use chrono::{NaiveDate, TimeZone as _, Utc};

use super::*;
use crate::telemetry::{
    schema::{HookAgent, HookOutcome, HookSurface, UtcDay, UtcSecond},
    state::IDENTIFIER_WINDOW_TEST_STATE,
    storage::{
        LockedStorage, Storage, TestWriteOperation, TestWrites,
        maintenance::MaintenanceOutcome,
        metrics::{MetricSnapshotUnavailable, PrepareMetricSnapshotError},
        paths::StoragePaths,
        private_state::{LoadStateError, OpenStateError},
        recording::{
            EventPublication, MetricPublication, MetricPublicationFailure, PersistRecordingError,
            PrepareRecordingError,
        },
    },
};

fn day(year: i32, month: u32, day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(year, month, day).unwrap())
}

fn completed_at(year: i32, month: u32, day: u32) -> UtcSecond {
    UtcSecond::from_datetime(Utc.with_ymd_and_hms(year, month, day, 10, 2, 11).unwrap())
}

fn invocation() -> HookInvocationMetricObservation<'static> {
    HookInvocationMetricObservation {
        agent: HookAgent::Claude,
        hook: HookSurface::UserPromptSubmit,
        outcome: HookOutcome::Ok,
        duration: Duration::from_millis(17),
        vendor_session_id: None,
        plugin_attempts: Vec::new(),
        extension: None,
    }
}

fn storage(temporary: &tempfile::TempDir, writes: TestWrites) -> LockedStorage<TestWrites> {
    LockedStorage::try_acquire_with_writer(temporary.path(), writes).unwrap()
}

fn plant_expired_event(paths: &StoragePaths) -> std::path::PathBuf {
    let path = paths.event_file(day(2026, 8, 10));
    fs::write(&path, b"old telemetry\n").unwrap();
    path
}

#[test]
fn hook_recording_runs_the_complete_locked_spine() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let accepted_day = day(2026, 9, 10);

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::Persisted {
            publication: PublishOutcome {
                event: EventPublication::NotRequested,
                metric: MetricPublication::Published,
            },
            maintenance: Some(MaintenanceOutcome::Completed { .. }),
        }
    ));
    assert_eq!(
        calls.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::MetricSnapshot,
        ]
    );
    assert!(storage.paths.state_file().is_file());
    assert!(storage.paths.metrics_file(accepted_day).is_file());
    assert!(!storage.paths.event_file(accepted_day).exists());
}

#[test]
fn hook_recording_entry_acquires_storage_and_runs_the_spine() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new(temporary.path());
    let accepted_day = day(2026, 9, 10);

    let outcome = record_hook_invocation(temporary.path(), completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::Persisted {
            publication: PublishOutcome {
                event: EventPublication::NotRequested,
                metric: MetricPublication::Published,
            },
            maintenance: Some(MaintenanceOutcome::Completed { .. }),
        }
    ));
    assert!(paths.state_file().is_file());
    assert!(paths.metrics_file(accepted_day).is_file());
    assert!(!paths.event_file(accepted_day).exists());
}

#[test]
fn publication_failure_still_reports_persisted_and_releases_maintenance() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::MetricSnapshot);
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let expired = plant_expired_event(&storage.paths);

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::Persisted {
            publication: PublishOutcome {
                event: EventPublication::NotRequested,
                metric: MetricPublication::Failed(MetricPublicationFailure::Write(
                    io::ErrorKind::Other
                )),
            },
            maintenance: Some(MaintenanceOutcome::Completed {
                removed_daily_files: 1,
                ..
            }),
        }
    ));
    assert_eq!(
        calls.calls(),
        [
            TestWriteOperation::PrivateState,
            TestWriteOperation::MetricSnapshot,
        ]
    );
    assert!(storage.paths.state_file().is_file());
    assert!(!expired.exists());
}

#[test]
fn persistence_failure_stops_before_publication_and_maintenance() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::failing(TestWriteOperation::PrivateState);
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let expired = plant_expired_event(&storage.paths);

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::Persistence(
            PersistRecordingError::Write(io::ErrorKind::Other)
        ))
    ));
    assert_eq!(calls.calls(), [TestWriteOperation::PrivateState]);
    assert!(!storage.paths.state_file().exists());
    assert!(!storage.paths.metrics_file(day(2026, 9, 10)).exists());
    assert!(expired.exists());
}

#[test]
fn unsafe_snapshot_drops_the_change_and_its_maintenance_attempt() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let paths = storage.paths.clone();
    let expired = plant_expired_event(&paths);
    let unsafe_snapshot = paths.metrics_file(day(2026, 9, 10));
    let unsafe_bytes = b"{\n";
    fs::write(&unsafe_snapshot, unsafe_bytes).unwrap();

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::MetricNotRequested(
            MetricNotRequestedReason::SnapshotUnavailable(
                MetricSnapshotUnavailable::MalformedLine { line: 1 }
            )
        ))
    ));
    assert!(calls.calls().is_empty());
    assert!(!paths.state_file().exists());
    assert!(!paths.event_file(day(2026, 9, 10)).exists());
    assert_eq!(fs::read(unsafe_snapshot).unwrap(), unsafe_bytes);
    assert!(expired.exists());
}

#[test]
fn malformed_private_state_stops_recording_and_cleanup() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let paths = storage.paths.clone();
    let malformed = b"version = 1\n\n[identity]\nkey = \"unfinished\n";
    fs::write(paths.state_file(), malformed).unwrap();
    let expired = plant_expired_event(&paths);

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::State(OpenStateError::Load(
            LoadStateError::Content(_)
        )))
    ));
    assert!(calls.calls().is_empty());
    assert_eq!(fs::read(paths.state_file()).unwrap(), malformed);
    assert!(!paths.metrics_file(day(2026, 9, 10)).exists());
    assert!(!paths.event_file(day(2026, 9, 10)).exists());
    assert!(expired.exists());
}

#[test]
fn contention_is_not_recorded_and_creates_only_the_lock_boundary() {
    let temporary = tempfile::tempdir().unwrap();
    let held = Storage::try_acquire(temporary.path()).unwrap();
    let paths = held.paths.clone();

    let outcome = record_hook_invocation(temporary.path(), completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::Lock(LockError::Contended))
    ));
    assert!(paths.lock_file().is_file());
    assert!(!paths.state_file().exists());
    assert!(!paths.metrics_file(day(2026, 9, 10)).exists());
    assert!(!paths.event_file(day(2026, 9, 10)).exists());
}

#[test]
fn a_day_before_the_high_water_mark_is_reported_without_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    fs::write(storage.paths.state_file(), IDENTIFIER_WINDOW_TEST_STATE).unwrap();

    let outcome = storage.record_hook_invocation(completed_at(2026, 8, 2), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::Observation(
            RecordingObservationError::BeforeHighWater {
                observed_day,
                latest_opened_day,
            }
        )) if observed_day == day(2026, 8, 2)
            && latest_opened_day == day(2026, 8, 3)
    ));
    assert!(calls.calls().is_empty());
    assert_eq!(
        fs::read_to_string(storage.paths.state_file()).unwrap(),
        IDENTIFIER_WINDOW_TEST_STATE
    );
}

#[test]
fn a_future_daily_file_initializes_the_high_water_mark_and_pauses_recording() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let future_day = day(2026, 9, 20);
    let observed_day = day(2026, 9, 10);
    let future_event = storage.paths.event_file(future_day);
    let future_bytes = b"future telemetry\n";
    fs::write(&future_event, future_bytes).unwrap();

    let outcome = storage.record_hook_invocation(completed_at(2026, 9, 10), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::Observation(
            RecordingObservationError::BeforeHighWater {
                observed_day: rejected_day,
                latest_opened_day,
            }
        )) if rejected_day == observed_day && latest_opened_day == future_day
    ));
    assert!(calls.calls().is_empty());
    assert!(!storage.paths.state_file().exists());
    assert!(!storage.paths.metrics_file(observed_day).exists());
    assert!(!storage.paths.event_file(observed_day).exists());
    assert_eq!(fs::read(future_event).unwrap(), future_bytes);
}

#[test]
fn a_day_before_the_identifier_window_is_reported_without_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let writes = TestWrites::default();
    let calls = writes.clone();
    let mut storage = storage(&temporary, writes);
    let source = IDENTIFIER_WINDOW_TEST_STATE.replacen(
        "identifier-window-anchor = \"2026-08-03\"",
        "identifier-window-anchor = \"2026-08-10\"",
        1,
    );
    fs::write(storage.paths.state_file(), &source).unwrap();

    let outcome = storage.record_hook_invocation(completed_at(2026, 8, 5), invocation());

    assert!(matches!(
        outcome,
        RecordingOutcome::NotRecorded(NotRecordedReason::Observation(
            RecordingObservationError::BeforeIdentifierWindow {
                observed_day,
                window_anchor,
            }
        )) if observed_day == day(2026, 8, 5)
            && window_anchor == day(2026, 8, 10)
    ));
    assert!(calls.calls().is_empty());
    assert_eq!(
        fs::read_to_string(storage.paths.state_file()).unwrap(),
        source
    );
}

#[test]
fn the_accepted_day_selects_the_metric_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage = storage(&temporary, TestWrites::default());
    fs::write(storage.paths.state_file(), IDENTIFIER_WINDOW_TEST_STATE).unwrap();
    let stale_snapshot = storage.paths.metrics_file(day(2026, 8, 3));
    let stale_bytes = b"{\n";
    fs::write(&stale_snapshot, stale_bytes).unwrap();

    let outcome = storage.record_hook_invocation(completed_at(2026, 8, 4), invocation());

    assert!(matches!(outcome, RecordingOutcome::Persisted { .. }));
    assert_eq!(fs::read(stale_snapshot).unwrap(), stale_bytes);
    assert!(storage.paths.metrics_file(day(2026, 8, 4)).is_file());
}

#[test]
fn preparation_projection_drops_foreign_serializer_errors() {
    let recognizable = "recognizable-private-serializer-detail";
    let metric_error = <serde_json::Error as serde::ser::Error>::custom(recognizable);
    let state_error = <toml::ser::Error as serde::ser::Error>::custom(recognizable);

    let metric = RecordingPreparationFailure::from(PrepareRecordingError::Metric(
        PrepareMetricSnapshotError::Serialize(metric_error),
    ));
    let state = RecordingPreparationFailure::from(PrepareRecordingError::PrivateState(
        StateEncodeError::Serialize(state_error),
    ));

    assert_eq!(metric, RecordingPreparationFailure::MetricSerialization);
    assert_eq!(state, RecordingPreparationFailure::StateSerialization);
    assert!(!format!("{metric:?}{state:?}").contains(recognizable));
    let failure = RecordingPreparationFailure::from(PrepareRecordingError::PrivateState(
        StateEncodeError::Invariant(StateInvariantError::MaintenanceAfterHighWater),
    ));

    assert_eq!(
        failure,
        RecordingPreparationFailure::StateInvariant(StateInvariantError::MaintenanceAfterHighWater)
    );
}

#[test]
fn metric_not_requested_uses_the_snapshot_diagnostic() {
    let reason =
        NotRecordedReason::MetricNotRequested(MetricNotRequestedReason::SnapshotUnavailable(
            MetricSnapshotUnavailable::MalformedLine { line: 3 },
        ));

    assert_eq!(
        reason.to_string(),
        "aggregate metric snapshot line 3 is malformed"
    );
}
