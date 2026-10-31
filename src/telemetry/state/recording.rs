//! Root private state paired with every unpublished part of one recording.

use std::{error::Error, fmt};

use super::{
    MaintenanceDue, TelemetryStateV1,
    aggregate::recording::{AggregateRecordingError, HookInvocationMetricObservation},
    lifecycle::{RecordingObservation, RecordingObservationBindingError},
};
use crate::telemetry::{
    schema::DroppedOperation,
    storage::{
        RecordingPersistenceAccess,
        events::EventBatch,
        metrics::{MetricSnapshot, MetricSnapshotUnavailable},
    },
};

/// Optional low-volume publication belonging to one state-produced change.
#[derive(Debug)]
pub(in crate::telemetry) struct EventPublicationRequest {
    batch: EventBatch,
    operation: DroppedOperation,
}

impl EventPublicationRequest {
    #[must_use]
    pub(in crate::telemetry) const fn new(batch: EventBatch, operation: DroppedOperation) -> Self {
        Self { batch, operation }
    }

    /// Move the batch and storage-limit classification into persistence.
    pub(in crate::telemetry) fn into_parts(self) -> (EventBatch, DroppedOperation) {
        (self.batch, self.operation)
    }
}

/// Aggregate publication selected while building one recording change.
#[derive(Debug)]
pub(in crate::telemetry) enum MetricRecording {
    Staged(MetricSnapshot),
    NotRequested(MetricNotRequestedReason),
}

/// Why one accepted operation deliberately carries no aggregate snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum MetricNotRequestedReason {
    NoAggregateRows,
    SnapshotUnavailable(MetricSnapshotUnavailable),
}

impl fmt::Display for MetricNotRequestedReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAggregateRows => {
                formatter.write_str("the operation has no aggregate metric rows")
            }
            Self::SnapshotUnavailable(reason) => reason.fmt(formatter),
        }
    }
}

/// Whether a state-produced change has any public data to publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ChangePublication {
    Present,
    None(MetricNotRequestedReason),
}

/// Complete private and public changes selected for one recording operation.
///
/// The state layer binds an accepted observation once and derives every event
/// and aggregate row before it can construct this value. Locked storage is the
/// only boundary allowed to separate the parts.
#[must_use = "a recording change must enter the storage transaction or be dropped"]
pub(in crate::telemetry) struct RecordingChange {
    state: TelemetryStateV1,
    metric: MetricRecording,
    event: Option<EventPublicationRequest>,
    maintenance_due: Option<MaintenanceDue>,
}

impl RecordingChange {
    /// Report whether this change has any public data to publish.
    #[must_use]
    pub(in crate::telemetry) const fn publication(&self) -> ChangePublication {
        match (&self.metric, &self.event) {
            (MetricRecording::Staged(_), _) | (_, Some(_)) => ChangePublication::Present,
            (MetricRecording::NotRequested(reason), None) => ChangePublication::None(*reason),
        }
    }

    /// Move the complete change into the locked persistence transaction.
    pub(in crate::telemetry) fn into_persistence_parts(
        self,
        _access: RecordingPersistenceAccess,
    ) -> (
        TelemetryStateV1,
        MetricRecording,
        Option<EventPublicationRequest>,
        Option<MaintenanceDue>,
    ) {
        (self.state, self.metric, self.event, self.maintenance_due)
    }

    /// Build deliberately correlated or uncorrelated transaction fixtures.
    #[cfg(test)]
    pub(in crate::telemetry) const fn from_parts_for_test(
        state: TelemetryStateV1,
        metric: MetricRecording,
        event: Option<EventPublicationRequest>,
        maintenance_due: Option<MaintenanceDue>,
    ) -> Self {
        Self {
            state,
            metric,
            event,
            maintenance_due,
        }
    }

    /// Add a low-volume request to a state-produced transaction fixture.
    #[cfg(test)]
    pub(in crate::telemetry) fn with_event_for_test(
        mut self,
        event: EventPublicationRequest,
    ) -> Self {
        self.event = Some(event);
        self
    }
}

impl TelemetryStateV1 {
    /// Stage one accepted hook invocation against this private state.
    ///
    /// A loaded snapshot is updated from a bound recording context. An unsafe
    /// snapshot produces no public rows and therefore needs no binding; the
    /// facade drops that change before persistence. In either case the state
    /// transition and maintenance token remain sealed inside the returned
    /// value.
    ///
    /// # Errors
    ///
    /// Returns the binding or invocation-wide aggregate failure.
    pub(in crate::telemetry) fn stage_hook_invocation(
        mut self,
        observation: RecordingObservation,
        snapshot: Result<MetricSnapshot, MetricSnapshotUnavailable>,
        invocation: HookInvocationMetricObservation<'_>,
    ) -> Result<RecordingChange, StageRecordingError> {
        let maintenance_due = self.schedule_maintenance(&observation);
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(reason) => {
                return Ok(RecordingChange {
                    state: self,
                    metric: MetricRecording::NotRequested(
                        MetricNotRequestedReason::SnapshotUnavailable(reason),
                    ),
                    event: None,
                    maintenance_due,
                });
            }
        };

        let aggregates = {
            let recording = self.identity.bind_recording_observation(observation);
            debug_assert!(
                recording.is_ok(),
                "BUG: nothing between observation and binding may change recording identity"
            );
            let recording = recording.map_err(StageRecordingError::Bind)?;
            self.aggregates
                .stage_hook_invocation(&recording, snapshot, invocation)
                .map_err(StageRecordingError::Aggregate)?
        };

        Ok(RecordingChange {
            state: self,
            metric: MetricRecording::Staged(aggregates.into_snapshot()),
            event: None,
            maintenance_due,
        })
    }
}

/// Failure while producing one state-owned recording change.
#[derive(Debug)]
pub(in crate::telemetry) enum StageRecordingError {
    Bind(RecordingObservationBindingError),
    Aggregate(AggregateRecordingError),
}

impl fmt::Display for StageRecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(_) => formatter.write_str("failed to bind telemetry recording state"),
            Self::Aggregate(_) => formatter.write_str("failed to stage aggregate recording"),
        }
    }
}

impl Error for StageRecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bind(error) => Some(error),
            Self::Aggregate(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{TimeZone as _, Utc};

    use super::*;
    use crate::telemetry::{
        schema::{HookAgent, HookOutcome, HookSurface, LowVolumeRow, StorageLimitV1, UtcSecond},
        state::IDENTIFIER_WINDOW_TEST_STATE,
    };

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

    #[test]
    fn unavailable_hook_snapshot_exposes_the_expected_drop_reason() {
        let mut state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
        let observation = state.observe_recording(completed_at()).unwrap();
        let reason = MetricSnapshotUnavailable::MalformedLine { line: 3 };

        let change = state
            .stage_hook_invocation(observation, Err(reason), invocation())
            .unwrap();

        assert_eq!(
            change.publication(),
            ChangePublication::None(MetricNotRequestedReason::SnapshotUnavailable(reason))
        );
    }

    #[test]
    fn no_aggregate_rows_without_an_event_is_distinguishable_from_snapshot_failure() {
        let state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
        let change = RecordingChange::from_parts_for_test(
            state,
            MetricRecording::NotRequested(MetricNotRequestedReason::NoAggregateRows),
            None,
            None,
        );

        assert_eq!(
            change.publication(),
            ChangePublication::None(MetricNotRequestedReason::NoAggregateRows)
        );
    }

    #[test]
    fn staged_metrics_and_low_volume_events_each_make_publication_present() {
        let day = completed_at().day();
        let metric_change = RecordingChange::from_parts_for_test(
            TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE),
            MetricRecording::Staged(MetricSnapshot::empty_for_test(day)),
            None,
            None,
        );
        let event = EventPublicationRequest::new(
            EventBatch::build(
                day,
                vec![LowVolumeRow::StorageLimit(StorageLimitV1::new(
                    day,
                    DroppedOperation::Command,
                ))],
            )
            .unwrap(),
            DroppedOperation::Command,
        );
        let event_change = RecordingChange::from_parts_for_test(
            TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE),
            MetricRecording::NotRequested(MetricNotRequestedReason::NoAggregateRows),
            Some(event),
            None,
        );

        assert_eq!(metric_change.publication(), ChangePublication::Present);
        assert_eq!(event_change.publication(), ChangePublication::Present);
    }
}
