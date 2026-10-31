//! Ordered persistence and best-effort publication for one recording.
//!
//! Preparation finishes every serialization before storage changes. Private
//! state is then replaced first and mints a lock-bound receipt. Only that
//! receipt can publish the prepared event and metric data, so publication
//! before persistence has no representation.
//!
//! Events publish before metrics because low-volume rows cannot be rebuilt,
//! while the cumulative metric snapshot can recover on a later operation. The
//! metric policy remeasures daily usage after the event attempt, making that
//! priority deterministic near the shared ceiling. Private state lives outside
//! the daily data directory and does not consume that allowance.

use std::{error::Error, fmt, io};

use super::{
    LockedStorage, RecordingPersistenceAccess, RecordingWrites,
    atomic::AtomicReplaceError,
    events::{AppendEventBatchError, EventBatch},
    limits::{EventSuppression, EventWrite},
    metrics::{
        MetricSuppression, MetricWrite, PrepareMetricSnapshotError, PreparedMetricSnapshot,
        ReplaceMetricSnapshotError,
    },
    private_state::PreparedPrivateState,
};
use crate::telemetry::{
    schema::{DroppedOperation, UtcDay},
    state::{StagedRecording, StateEncodeError},
};

/// Optional low-volume publication belonging to one staged recording.
#[derive(Debug)]
pub(super) struct EventPublicationRequest {
    batch: EventBatch,
    operation: DroppedOperation,
}

impl EventPublicationRequest {
    #[must_use]
    pub(super) const fn new(batch: EventBatch, operation: DroppedOperation) -> Self {
        Self { batch, operation }
    }
}

/// Complete bytes and policy decisions prepared before private-state storage.
#[derive(Debug)]
#[must_use = "a prepared recording must be persisted or explicitly dropped"]
pub(super) struct PreparedRecording {
    private_state: PreparedPrivateState,
    event: Option<EventWrite>,
    metric: PreparedMetricSnapshot,
    stopped_day: Option<UtcDay>,
}

impl<Writer> LockedStorage<Writer> {
    /// Prepare one staged recording without changing any file.
    ///
    /// The state and aggregate snapshot correspond by construction: the state
    /// coordinator is their only producer. Day checks defend that boundary and
    /// any future producer that may be added.
    ///
    /// # Errors
    ///
    /// Returns a day mismatch or a private-state or metric serialization
    /// failure. No file has changed when this method returns an error.
    pub(super) fn prepare_recording(
        &self,
        staged: StagedRecording,
        event: Option<EventPublicationRequest>,
    ) -> Result<PreparedRecording, PrepareRecordingError> {
        let access = RecordingPersistenceAccess::new();
        let (mut state, snapshot) = staged.into_persistence_parts(access);
        let state_day = state.latest_opened_day();
        let metric = snapshot.prepare().map_err(PrepareRecordingError::Metric)?;
        if metric.day() != state_day {
            return Err(PrepareRecordingError::MetricDayMismatch {
                state_day,
                metric_day: metric.day(),
            });
        }

        let event = event
            .map(|request| {
                if request.batch.day() != state_day {
                    return Err(PrepareRecordingError::EventDayMismatch {
                        state_day,
                        event_day: request.batch.day(),
                    });
                }

                Ok(self.plan_event_write(
                    request.batch,
                    request.operation,
                    state.storage_limit_day(),
                ))
            })
            .transpose()?;

        if event
            .as_ref()
            .is_some_and(EventWrite::stops_event_recording)
        {
            state.stop_event_recording(state_day);
        }
        let stopped_day = state.storage_limit_day();
        let private_state =
            PreparedPrivateState::new(&state).map_err(PrepareRecordingError::PrivateState)?;

        Ok(PreparedRecording {
            private_state,
            event,
            metric,
            stopped_day,
        })
    }
}

/// Failure while preparing a recording before any write.
#[derive(Debug)]
pub(super) enum PrepareRecordingError {
    MetricDayMismatch {
        state_day: UtcDay,
        metric_day: UtcDay,
    },
    EventDayMismatch {
        state_day: UtcDay,
        event_day: UtcDay,
    },
    Metric(PrepareMetricSnapshotError),
    PrivateState(StateEncodeError),
}

impl fmt::Display for PrepareRecordingError {
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
            Self::Metric(_) => formatter.write_str("failed to prepare aggregate metric snapshot"),
            Self::PrivateState(_) => {
                formatter.write_str("failed to prepare telemetry private state")
            }
        }
    }
}

impl Error for PrepareRecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metric(error) => Some(error),
            Self::PrivateState(error) => Some(error),
            Self::MetricDayMismatch { .. } | Self::EventDayMismatch { .. } => None,
        }
    }
}

impl<Writer: RecordingWrites> LockedStorage<Writer> {
    /// Persist private state and mint the only value allowed to publish.
    ///
    /// # Errors
    ///
    /// Returns the bounded atomic replacement stage and I/O error kind. No
    /// public telemetry file is touched after a private-state failure.
    pub(super) fn persist_recording(
        &mut self,
        prepared: PreparedRecording,
    ) -> Result<PersistedRecording<'_, Writer>, PersistRecordingError> {
        self.replace_private_state(&prepared.private_state)
            .map_err(PersistRecordingError::from)?;

        Ok(PersistedRecording {
            storage: self,
            event: prepared.event,
            metric: prepared.metric,
            stopped_day: prepared.stopped_day,
            publication_attempted: false,
        })
    }
}

/// Private state was not persisted, so no publication receipt was minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PersistRecordingError {
    Create(io::ErrorKind),
    Write(io::ErrorKind),
    Persist(io::ErrorKind),
}

impl From<AtomicReplaceError> for PersistRecordingError {
    fn from(error: AtomicReplaceError) -> Self {
        match error {
            AtomicReplaceError::Create(error) => Self::Create(error.kind()),
            AtomicReplaceError::Write(error) => Self::Write(error.kind()),
            AtomicReplaceError::Persist(error) => Self::Persist(error.kind()),
        }
    }
}

impl fmt::Display for PersistRecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Create(_) => "failed to create temporary telemetry private state",
            Self::Write(_) => "failed to write temporary telemetry private state",
            Self::Persist(_) => "failed to replace telemetry private state",
        })
    }
}

impl Error for PersistRecordingError {}

/// Proof that private state was replaced while this storage lock is held.
///
/// The lifetime prevents the receipt from surviving the locked storage value
/// that minted it. Dropping it without calling [`Self::publish`] is a recording
/// bug and is reported loudly.
#[must_use = "persisted telemetry must attempt publication before the lock is released"]
pub(super) struct PersistedRecording<'storage, Writer> {
    storage: &'storage mut LockedStorage<Writer>,
    event: Option<EventWrite>,
    metric: PreparedMetricSnapshot,
    stopped_day: Option<UtcDay>,
    publication_attempted: bool,
}

impl<Writer: RecordingWrites> PersistedRecording<'_, Writer> {
    /// Attempt every prepared public write and report their independent result.
    #[must_use]
    pub(super) fn publish(mut self) -> PublishOutcome {
        // Set this before any writer call: a panic during publication is itself
        // the signal and must not be misreported by Drop as a skipped attempt.
        self.publication_attempted = true;

        let event = match &self.event {
            None => EventPublication::NotRequested,
            Some(EventWrite::Suppressed(reason)) => EventPublication::Suppressed(*reason),
            Some(EventWrite::Append(batch) | EventWrite::Marker(batch)) => {
                match self.storage.append_event_batch(batch) {
                    Ok(()) => EventPublication::Published,
                    Err(error) => EventPublication::Failed(error.into()),
                }
            }
        };

        let metric = match self
            .storage
            .plan_metric_write(&self.metric, self.stopped_day)
        {
            MetricWrite::Suppressed(reason) => MetricPublication::Suppressed(reason),
            MetricWrite::Replace => match self.storage.replace_metric_snapshot(&self.metric) {
                Ok(()) => MetricPublication::Published,
                Err(error) => MetricPublication::Failed(error.into()),
            },
        };

        PublishOutcome { event, metric }
    }
}

impl<Writer> Drop for PersistedRecording<'_, Writer> {
    fn drop(&mut self) {
        if self.publication_attempted || std::thread::panicking() {
            return;
        }

        tracing::warn!("telemetry: persisted recording dropped before publication was attempted");
        debug_assert!(
            false,
            "BUG: persisted telemetry recording dropped before publication"
        );
    }
}

/// Independent results of the two best-effort public writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PublishOutcome {
    pub(super) event: EventPublication,
    pub(super) metric: MetricPublication,
}

/// Result of the optional low-volume event publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventPublication {
    NotRequested,
    Published,
    Suppressed(EventSuppression),
    Failed(EventPublicationFailure),
}

/// Bounded event-append failure retained for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventPublicationFailure {
    Prepare(io::ErrorKind),
    InspectTail(io::ErrorKind),
    RepairTail(io::ErrorKind),
    Write(io::ErrorKind),
}

impl From<AppendEventBatchError> for EventPublicationFailure {
    fn from(error: AppendEventBatchError) -> Self {
        match error {
            AppendEventBatchError::Prepare(error) => Self::Prepare(error.kind()),
            AppendEventBatchError::InspectTail(error) => Self::InspectTail(error.kind()),
            AppendEventBatchError::RepairTail(error) => Self::RepairTail(error.kind()),
            AppendEventBatchError::Write(error) => Self::Write(error.kind()),
        }
    }
}

/// Result of the aggregate metric publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MetricPublication {
    Published,
    Suppressed(MetricSuppression),
    Failed(MetricPublicationFailure),
}

/// Bounded metric-replacement failure retained for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MetricPublicationFailure {
    Create(io::ErrorKind),
    Write(io::ErrorKind),
    Persist(io::ErrorKind),
}

impl From<ReplaceMetricSnapshotError> for MetricPublicationFailure {
    fn from(error: ReplaceMetricSnapshotError) -> Self {
        error.into_atomic().into()
    }
}

impl From<AtomicReplaceError> for MetricPublicationFailure {
    fn from(error: AtomicReplaceError) -> Self {
        match error {
            AtomicReplaceError::Create(error) => Self::Create(error.kind()),
            AtomicReplaceError::Write(error) => Self::Write(error.kind()),
            AtomicReplaceError::Persist(error) => Self::Persist(error.kind()),
        }
    }
}

#[cfg(test)]
mod tests;
