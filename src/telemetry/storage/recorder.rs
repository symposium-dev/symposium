//! Lock-bound orchestration for complete telemetry recording operations.
//!
//! The caller decides whether telemetry is enabled. Once invoked, this module
//! makes one non-waiting lock attempt and keeps load, staging, persistence,
//! publication, and maintenance under that same lock. Ordinary failures become
//! a typed outcome; invariant assertions remain loud in debug builds.

use std::{fmt, path::Path};

use super::{
    LockError, LockedStorage, RecordingWrites, Storage,
    maintenance::MaintenanceOutcome,
    metrics::{MetricSnapshotUnavailable, PrepareMetricSnapshotError},
    private_state::OpenStateError,
    recording::{PersistRecordingError, PrepareRecordingError, PublishOutcome},
};
use crate::telemetry::{
    schema::{UtcDay, UtcSecond},
    state::{
        ChangePublication, HookInvocationMetricObservation, MetricNotRequestedReason,
        RecordingChange, RecordingObservationError, StageRecordingError, StateEncodeError,
        StateInvariantError,
    },
};

/// Result of one best-effort recording operation.
#[derive(Debug)]
#[must_use = "a recording outcome must be reported or deliberately ignored"]
pub(in crate::telemetry) enum RecordingOutcome {
    /// Nothing reached private or public storage.
    NotRecorded(NotRecordedReason),
    /// Private state was replaced; public writes report independently.
    Persisted {
        publication: PublishOutcome,
        maintenance: Option<MaintenanceOutcome>,
    },
}

/// Why one recording operation changed no durable state or public data.
#[derive(Debug)]
pub(in crate::telemetry) enum NotRecordedReason {
    Lock(LockError),
    State(OpenStateError),
    Observation(RecordingObservationError),
    MetricNotRequested(MetricNotRequestedReason),
    Staging(StageRecordingError),
    Preparation(RecordingPreparationFailure),
    Persistence(PersistRecordingError),
}

/// Content-free preparation failure safe to retain in recorder diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum RecordingPreparationFailure {
    MetricDayMismatch {
        state_day: UtcDay,
        metric_day: UtcDay,
    },
    EventDayMismatch {
        state_day: UtcDay,
        event_day: UtcDay,
    },
    MetricSerialization,
    StateInvariant(StateInvariantError),
    StateSerialization,
}

impl fmt::Display for RecordingPreparationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MetricDayMismatch {
                state_day,
                metric_day,
            } => write!(
                formatter,
                "private state belongs to {state_day}, not metric day {metric_day}"
            ),
            Self::EventDayMismatch {
                state_day,
                event_day,
            } => write!(
                formatter,
                "private state belongs to {state_day}, not event day {event_day}"
            ),
            Self::MetricSerialization => {
                formatter.write_str("failed to serialize aggregate metric snapshot")
            }
            Self::StateInvariant(reason) => reason.fmt(formatter),
            Self::StateSerialization => {
                formatter.write_str("failed to serialize telemetry private state")
            }
        }
    }
}

impl From<PrepareRecordingError> for RecordingPreparationFailure {
    fn from(error: PrepareRecordingError) -> Self {
        match error {
            PrepareRecordingError::MetricDayMismatch {
                state_day,
                metric_day,
            } => Self::MetricDayMismatch {
                state_day,
                metric_day,
            },
            PrepareRecordingError::EventDayMismatch {
                state_day,
                event_day,
            } => Self::EventDayMismatch {
                state_day,
                event_day,
            },
            PrepareRecordingError::Metric(PrepareMetricSnapshotError::Serialize(_)) => {
                Self::MetricSerialization
            }
            PrepareRecordingError::PrivateState(StateEncodeError::Invariant(reason)) => {
                Self::StateInvariant(reason)
            }
            PrepareRecordingError::PrivateState(StateEncodeError::Serialize(_)) => {
                Self::StateSerialization
            }
        }
    }
}

impl fmt::Display for NotRecordedReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lock(error) => error.fmt(formatter),
            Self::State(error) => error.fmt(formatter),
            Self::Observation(error) => error.fmt(formatter),
            Self::MetricNotRequested(reason) => reason.fmt(formatter),
            Self::Staging(error) => error.fmt(formatter),
            Self::Preparation(error) => error.fmt(formatter),
            Self::Persistence(error) => error.fmt(formatter),
        }
    }
}

/// Record one completed hook invocation without waiting for another recorder.
///
/// This entry point does not inspect telemetry configuration. Its eventual
/// caller must invoke it only after current consent has enabled recording.
pub(in crate::telemetry) fn record_hook_invocation(
    config_dir: &Path,
    completed_at: UtcSecond,
    invocation: HookInvocationMetricObservation<'_>,
) -> RecordingOutcome {
    let mut storage = match Storage::try_acquire(config_dir) {
        Ok(storage) => storage,
        Err(error) => return not_recorded(NotRecordedReason::Lock(error)),
    };

    storage.record_hook_invocation(completed_at, invocation)
}

impl<Writer: RecordingWrites> LockedStorage<Writer> {
    /// Run the hook-recording spine while this value proves the lock is held.
    fn record_hook_invocation(
        &mut self,
        completed_at: UtcSecond,
        invocation: HookInvocationMetricObservation<'_>,
    ) -> RecordingOutcome {
        let mut state = match self.load_or_initialize_state(completed_at.day()) {
            Ok(state) => state,
            Err(error) => return not_recorded(NotRecordedReason::State(error)),
        };
        let observation = match state.observe_recording(completed_at) {
            Ok(observation) => observation,
            Err(error) => return not_recorded(NotRecordedReason::Observation(error)),
        };
        let accepted_day = state.latest_opened_day();

        // Snapshot failure is the aggregate half's fail-closed value. Do not
        // use `?` here: later recording entries may still carry an event.
        let snapshot = match self.load_metric_snapshot(accepted_day) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => Err(MetricSnapshotUnavailable::from(error)),
        };
        let change = match state.stage_hook_invocation(observation, snapshot, invocation) {
            Ok(change) => change,
            Err(error) => return not_recorded(NotRecordedReason::Staging(error)),
        };

        self.persist_change(change)
    }

    /// Finish the storage transaction shared by every recording entry point.
    fn persist_change(&mut self, change: RecordingChange) -> RecordingOutcome {
        if let ChangePublication::None(reason) = change.publication() {
            return not_recorded(NotRecordedReason::MetricNotRequested(reason));
        }

        let prepared = match self.prepare_recording(change) {
            Ok(prepared) => prepared,
            Err(error) => {
                return not_recorded(NotRecordedReason::Preparation(error.into()));
            }
        };
        let persisted = match self.persist_recording(prepared) {
            Ok(persisted) => persisted,
            Err(error) => return not_recorded(NotRecordedReason::Persistence(error)),
        };
        let (publication, maintenance_due) = persisted.publish();
        let maintenance = maintenance_due.map(|due| self.maintain(due));

        RecordingOutcome::Persisted {
            publication,
            maintenance,
        }
    }
}

fn not_recorded(reason: NotRecordedReason) -> RecordingOutcome {
    tracing::debug!(
        reason = %reason,
        detail = ?reason,
        "telemetry: recording operation was not recorded"
    );
    RecordingOutcome::NotRecorded(reason)
}

#[cfg(test)]
mod tests;
