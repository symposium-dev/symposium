//! Atomic write-side updates for extension-invocation aggregate rows.

use std::fmt;

use super::ExtensionInvocationMetricsV1;
use crate::telemetry::{
    schema::{
        SchemaVersion, SymposiumVersion, UtcDay,
        agent::{HookAgent, VendorSessionId, derive_session_id},
        extension_invocation::ExtensionInvocationPhase,
    },
    state::{
        BoundRecordingObservation, ExtensionSessionCountBaseline, ExtensionSessionCountSnapshot,
        ExtensionSessionCountUpdateError, SelectedExtensionInvocationAggregate,
    },
};

/// One normalized skill-invocation observation added to an aggregate row.
///
/// The admitted selection supplies the agent and target bucket. This value
/// carries only the independently observed phase and optional raw session
/// identifier needed for the matching session set.
// Intentionally omit `Debug`: this value borrows a raw vendor session id.
#[derive(Clone, Copy)]
pub(in crate::telemetry) struct ExtensionInvocationMetricObservation<'a> {
    pub(in crate::telemetry) phase: ExtensionInvocationPhase,
    pub(in crate::telemetry) vendor_session_id: Option<&'a VendorSessionId>,
}

impl ExtensionInvocationMetricsV1 {
    /// Start an aggregate row with its first normalized skill observation.
    ///
    /// The selected private-state entry supplies the stable row identifier,
    /// agent, admitted target identity, and session tracker together. The
    /// recording is used only to verify the selected identifier epoch and to
    /// derive an optional scoped session identifier.
    ///
    /// The private entry may already contain contributions when private state
    /// survived a failed snapshot write. Starting the missing row reconciles
    /// that mismatch and publishes incomplete session counts.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionInvocationMetricsUpdateError`] when the supplied
    /// context selects another aggregate or a counter cannot represent the
    /// observation. Private state remains unchanged on failure.
    #[must_use = "a failed extension-invocation metric update must be dropped"]
    pub(in crate::telemetry) fn new(
        recording: &BoundRecordingObservation<'_>,
        observation: ExtensionInvocationMetricObservation<'_>,
        selected: SelectedExtensionInvocationAggregate<'_>,
    ) -> Result<Self, ExtensionInvocationMetricsUpdateError> {
        let bucket = selected.bucket();
        let mut row = Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: selected.event_id(),
            day: selected.day(),
            symposium: SymposiumVersion::current(),
            agent: selected.agent(),
            target_scope: bucket.scope(),
            target: bucket.target().cloned(),
            unnamed_reason: bucket.unnamed_reason(),
            attempted: 0,
            completed: 0,
            failed: 0,
            session_counts_complete: false,
            identified_sessions: None,
            identified_sessions_completed: None,
            extension_subject: bucket.extension_subject(),
        };

        row.checked_record(recording, observation, selected)?;
        Ok(row)
    }

    /// Add one normalized skill observation without partially changing its
    /// row or private session tracker.
    ///
    /// The row counter is updated on a clone first. Updating the private
    /// tracker is the final fallible operation and is itself transactional;
    /// applying its snapshot and replacing the row are then infallible. The
    /// snapshot is applied for every phase, including `failed`, so an earlier
    /// incomplete tracker cannot leave stale complete counts in the row.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionInvocationMetricsUpdateError`] when the recording
    /// or selected private state belongs to another aggregate, or when a
    /// counter would overflow. Both inputs remain unchanged on failure.
    #[must_use = "a failed extension-invocation metric update must be dropped"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        observation: ExtensionInvocationMetricObservation<'_>,
        mut selected: SelectedExtensionInvocationAggregate<'_>,
    ) -> Result<(), ExtensionInvocationMetricsUpdateError> {
        self.ensure_selected_by(recording, &selected)?;

        let baseline = ExtensionSessionCountBaseline {
            attempted: self.attempted,
            completed: self.completed,
        };
        let mut next = self.clone();
        let counter = match observation.phase {
            ExtensionInvocationPhase::Attempted => &mut next.attempted,
            ExtensionInvocationPhase::Completed => &mut next.completed,
            ExtensionInvocationPhase::Failed => &mut next.failed,
        };
        *counter = counter.checked_add(1).ok_or(
            ExtensionInvocationMetricsUpdateError::PhaseCountOverflow {
                phase: observation.phase,
            },
        )?;

        let session_id = match observation.phase {
            ExtensionInvocationPhase::Attempted | ExtensionInvocationPhase::Completed => {
                derive_session_id(
                    recording.identifier_window_scope(),
                    HookAgent::from(selected.agent()),
                    observation.vendor_session_id,
                )
            }
            ExtensionInvocationPhase::Failed => None,
        };
        let session_counts = selected.session_counts();
        // The matching row counter increment succeeded above, so the
        // tracker's contribution increment cannot overflow here.
        session_counts.checked_record(baseline, session_id, observation.phase)?;
        next.apply_session_counts(session_counts.snapshot());

        *self = next;
        Ok(())
    }

    fn ensure_selected_by(
        &self,
        recording: &BoundRecordingObservation<'_>,
        selected: &SelectedExtensionInvocationAggregate<'_>,
    ) -> Result<(), ExtensionInvocationMetricsUpdateError> {
        if !selected.matches_recording(recording) {
            return Err(ExtensionInvocationMetricsUpdateError::RecordingContextChanged);
        }
        if self.day != selected.day() {
            return Err(ExtensionInvocationMetricsUpdateError::DayChanged {
                row_day: self.day,
                selected_day: selected.day(),
            });
        }
        if self.agent != selected.agent() {
            return Err(ExtensionInvocationMetricsUpdateError::AgentChanged);
        }

        let bucket = selected.bucket();
        if self.target_scope != bucket.scope()
            || self.target.as_ref() != bucket.target()
            || self.unnamed_reason != bucket.unnamed_reason()
            || self.extension_subject != bucket.extension_subject()
        {
            return Err(ExtensionInvocationMetricsUpdateError::TargetIdentityChanged);
        }
        if self.event_id != selected.event_id() {
            return Err(ExtensionInvocationMetricsUpdateError::RowIdentifierChanged);
        }

        Ok(())
    }

    fn apply_session_counts(&mut self, snapshot: ExtensionSessionCountSnapshot) {
        match snapshot {
            ExtensionSessionCountSnapshot::Complete {
                identified_sessions,
                identified_sessions_completed,
            } => {
                self.session_counts_complete = true;
                self.identified_sessions = Some(identified_sessions);
                self.identified_sessions_completed = Some(identified_sessions_completed);
            }
            ExtensionSessionCountSnapshot::Incomplete => {
                self.session_counts_complete = false;
                self.identified_sessions = None;
                self.identified_sessions_completed = None;
            }
        }
    }
}

/// An extension-invocation aggregate update that cannot be represented safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionInvocationMetricsUpdateError {
    RecordingContextChanged,
    DayChanged {
        row_day: UtcDay,
        selected_day: UtcDay,
    },
    AgentChanged,
    TargetIdentityChanged,
    RowIdentifierChanged,
    PhaseCountOverflow {
        phase: ExtensionInvocationPhase,
    },
    // Defensive propagation: the matching row counter is incremented first,
    // so the tracker's corresponding contribution cannot currently overflow.
    SessionCounts(ExtensionSessionCountUpdateError),
}

impl From<ExtensionSessionCountUpdateError> for ExtensionInvocationMetricsUpdateError {
    fn from(error: ExtensionSessionCountUpdateError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for ExtensionInvocationMetricsUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordingContextChanged => formatter.write_str(
                "extension-invocation private state belongs to another recording context",
            ),
            Self::DayChanged {
                row_day,
                selected_day,
            } => write!(
                formatter,
                "extension-invocation row belongs to {row_day}, not selected day {selected_day}"
            ),
            Self::AgentChanged => {
                formatter.write_str("extension-invocation row belongs to another agent")
            }
            Self::TargetIdentityChanged => {
                formatter.write_str("extension-invocation row belongs to another target bucket")
            }
            Self::RowIdentifierChanged => formatter
                .write_str("extension-invocation private state belongs to another aggregate row"),
            Self::PhaseCountOverflow { phase } => write!(
                formatter,
                "extension {} count overflows u64",
                phase.counter_name()
            ),
            Self::SessionCounts(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtensionInvocationMetricsUpdateError {}

#[cfg(test)]
mod tests;
