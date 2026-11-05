//! Root private state paired with its unpublished aggregate snapshot.

use std::{error::Error, fmt};

use super::{
    TelemetryStateV1,
    aggregate::recording::{
        AggregateRecordingError, HookInvocationMetricObservation, StagedAggregateRecording,
    },
    lifecycle::{RecordingObservationBindingError, RecordingObservationError},
};
use crate::telemetry::{
    schema::UtcSecond,
    storage::{RecordingPersistenceAccess, metrics::MetricSnapshot},
};

/// Complete private state and aggregate rows staged for one recording.
///
/// The aggregate snapshot was derived from the stores inside `state`. The
/// storage transaction is the only boundary allowed to separate the pair.
#[must_use = "a staged recording must enter the storage transaction or be dropped"]
pub(in crate::telemetry) struct StagedRecording {
    state: TelemetryStateV1,
    aggregates: StagedAggregateRecording,
}

impl StagedRecording {
    /// Move both halves into the locked persistence transaction.
    #[must_use]
    pub(in crate::telemetry) fn into_persistence_parts(
        self,
        _access: RecordingPersistenceAccess,
    ) -> (TelemetryStateV1, MetricSnapshot) {
        (self.state, self.aggregates.into_snapshot())
    }

    /// Build a deliberately uncorrelated pair for transaction boundary tests.
    #[cfg(test)]
    pub(in crate::telemetry) const fn from_parts_for_test(
        state: TelemetryStateV1,
        snapshot: MetricSnapshot,
    ) -> Self {
        Self {
            state,
            aggregates: StagedAggregateRecording::from_snapshot_for_test(snapshot),
        }
    }
}

impl TelemetryStateV1 {
    /// Apply and stage one complete hook invocation against this private state.
    ///
    /// The returned value keeps the mutated state and its derived snapshot
    /// together. Neither half is publishable until storage persists the state.
    ///
    /// # Errors
    ///
    /// Returns the lifecycle, binding, or invocation-wide aggregate failure.
    pub(in crate::telemetry) fn stage_hook_invocation(
        mut self,
        completed_at: UtcSecond,
        snapshot: MetricSnapshot,
        invocation: HookInvocationMetricObservation<'_>,
    ) -> Result<StagedRecording, StageRecordingError> {
        let observation = self
            .observe_recording(completed_at)
            .map_err(StageRecordingError::Observe)?;
        let aggregates = {
            let recording = self.identity.bind_recording_observation(observation);
            debug_assert!(
                recording.is_ok(),
                "BUG: an immediately bound recording transition must match its state"
            );
            let recording = recording.map_err(StageRecordingError::Bind)?;
            self.aggregates
                .stage_hook_invocation(&recording, snapshot, invocation)
                .map_err(StageRecordingError::Aggregate)?
        };

        Ok(StagedRecording {
            state: self,
            aggregates,
        })
    }
}

/// Failure while producing one state-and-snapshot staging unit.
#[derive(Debug)]
pub(in crate::telemetry) enum StageRecordingError {
    Observe(RecordingObservationError),
    Bind(RecordingObservationBindingError),
    Aggregate(AggregateRecordingError),
}

impl fmt::Display for StageRecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Observe(_) => formatter.write_str("failed to select telemetry recording state"),
            Self::Bind(_) => formatter.write_str("failed to bind telemetry recording state"),
            Self::Aggregate(_) => formatter.write_str("failed to stage aggregate recording"),
        }
    }
}

impl Error for StageRecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Observe(error) => Some(error),
            Self::Bind(error) => Some(error),
            Self::Aggregate(error) => Some(error),
        }
    }
}
