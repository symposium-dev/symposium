//! Bounded private session sets for extension-invocation aggregates.

mod admission;

pub(in crate::telemetry) use admission::SelectedExtensionInvocationAggregate;

#[cfg(test)]
pub(in crate::telemetry) use admission::ExtensionInvocationAggregateStore;

use std::{collections::BTreeSet, fmt};

use crate::telemetry::{
    identity::SessionId,
    schema::{ExtensionInvocationPhase, MAX_IDENTIFIED_SESSIONS},
};

use super::set_len;

/// Snapshot counters used to reconcile one private tracker with its row.
///
/// Named fields keep the independent attempted and completed baselines from
/// being transposed at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionSessionCountBaseline {
    pub(in crate::telemetry) attempted: u64,
    pub(in crate::telemetry) completed: u64,
}

impl ExtensionSessionCountBaseline {
    const fn counter(self, phase: TrackedPhase) -> u64 {
        match phase {
            TrackedPhase::Attempted => self.attempted,
            TrackedPhase::Completed => self.completed,
        }
    }
}

/// Complete or permanently incomplete invocation-session sets.
#[derive(Clone, PartialEq, Eq)]
enum TrackedExtensionSessions {
    Complete {
        attempted: BTreeSet<SessionId>,
        completed: BTreeSet<SessionId>,
    },
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedPhase {
    Attempted,
    Completed,
}

impl TryFrom<ExtensionInvocationPhase> for TrackedPhase {
    type Error = ();

    fn try_from(phase: ExtensionInvocationPhase) -> Result<Self, Self::Error> {
        match phase {
            ExtensionInvocationPhase::Attempted => Ok(Self::Attempted),
            ExtensionInvocationPhase::Completed => Ok(Self::Completed),
            ExtensionInvocationPhase::Failed => Err(()),
        }
    }
}

impl From<TrackedPhase> for ExtensionInvocationPhase {
    fn from(phase: TrackedPhase) -> Self {
        match phase {
            TrackedPhase::Attempted => Self::Attempted,
            TrackedPhase::Completed => Self::Completed,
        }
    }
}

impl TrackedExtensionSessions {
    fn record(&mut self, phase: TrackedPhase, session_id: Option<SessionId>) {
        let Some(session_id) = session_id else {
            *self = Self::Incomplete;
            return;
        };
        let Self::Complete {
            attempted,
            completed,
        } = self
        else {
            return;
        };
        let sessions = match phase {
            TrackedPhase::Attempted => attempted,
            TrackedPhase::Completed => completed,
        };

        if !sessions.contains(&session_id) && set_len(sessions) >= MAX_IDENTIFIED_SESSIONS {
            *self = Self::Incomplete;
            return;
        }

        sessions.insert(session_id);
    }
}

/// Private session-count state for one extension-invocation aggregate row.
///
/// Raw and keyed session identifiers never enter the aggregate row. Once an
/// attempted or completed observation lacks an identifier, either set exceeds
/// the limit, or state disagrees with the snapshot counters, both sets are
/// discarded for the rest of this tracker's lifetime.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionSessionCountTracker<K> {
    key: K,
    attempted_contributions: u64,
    completed_contributions: u64,
    sessions: TrackedExtensionSessions,
}

impl<K> fmt::Debug for ExtensionSessionCountTracker<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ExtensionSessionCountTracker");
        debug
            .field("attempted_contributions", &self.attempted_contributions)
            .field("completed_contributions", &self.completed_contributions);

        match &self.sessions {
            TrackedExtensionSessions::Complete {
                attempted,
                completed,
            } => debug
                .field("session_counts_complete", &true)
                .field("identified_sessions", &attempted.len())
                .field("identified_sessions_completed", &completed.len()),
            TrackedExtensionSessions::Incomplete => debug.field("session_counts_complete", &false),
        };

        debug.finish()
    }
}

impl<K> ExtensionSessionCountTracker<K> {
    /// Start private session tracking for one aggregate key.
    #[must_use]
    pub(in crate::telemetry) const fn new(key: K) -> Self {
        Self {
            key,
            attempted_contributions: 0,
            completed_contributions: 0,
            sessions: TrackedExtensionSessions::Complete {
                attempted: BTreeSet::new(),
                completed: BTreeSet::new(),
            },
        }
    }

    /// Return the aggregate key this private state belongs to.
    #[must_use]
    const fn key(&self) -> &K {
        &self.key
    }

    fn counter_mut(&mut self, phase: TrackedPhase) -> &mut u64 {
        match phase {
            TrackedPhase::Attempted => &mut self.attempted_contributions,
            TrackedPhase::Completed => &mut self.completed_contributions,
        }
    }

    fn reconcile(&mut self, baseline: ExtensionSessionCountBaseline) {
        if self.attempted_contributions != baseline.attempted
            || self.completed_contributions != baseline.completed
        {
            self.sessions = TrackedExtensionSessions::Incomplete;
        }

        self.attempted_contributions = baseline.attempted;
        self.completed_contributions = baseline.completed;
    }

    /// Add one phase observation without partially changing the tracker.
    ///
    /// Missing identifiers and a 257th distinct identifier make both session
    /// sets incomplete but do not reject attempted or completed observations.
    /// A baseline mismatch has the same all-or-nothing result. Failed
    /// observations feed neither set and leave this tracker unchanged,
    /// including when they do not carry a session identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionSessionCountUpdateError::ContributionCountOverflow`]
    /// when the selected phase count cannot be incremented. The tracker is
    /// unchanged on failure.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        baseline: ExtensionSessionCountBaseline,
        session_id: Option<SessionId>,
        phase: ExtensionInvocationPhase,
    ) -> Result<(), ExtensionSessionCountUpdateError> {
        let Ok(tracked_phase) = TrackedPhase::try_from(phase) else {
            return Ok(());
        };
        let next_contributions = baseline
            .counter(tracked_phase)
            .checked_add(1)
            .ok_or(ExtensionSessionCountUpdateError::ContributionCountOverflow { phase })?;

        self.reconcile(baseline);
        self.sessions.record(tracked_phase, session_id);
        *self.counter_mut(tracked_phase) = next_contributions;

        Ok(())
    }

    /// Return the aggregate fields represented by the current private sets.
    #[must_use]
    pub(in crate::telemetry) fn snapshot(&self) -> ExtensionSessionCountSnapshot {
        match &self.sessions {
            TrackedExtensionSessions::Complete {
                attempted,
                completed,
            } => ExtensionSessionCountSnapshot::Complete {
                identified_sessions: set_len(attempted),
                identified_sessions_completed: set_len(completed),
            },
            TrackedExtensionSessions::Incomplete => ExtensionSessionCountSnapshot::Incomplete,
        }
    }
}

/// Session-count fields supplied to one extension-invocation aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionSessionCountSnapshot {
    Complete {
        identified_sessions: u64,
        identified_sessions_completed: u64,
    },
    Incomplete,
}

/// An extension-invocation session-count update that private state cannot
/// represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionSessionCountUpdateError {
    ContributionCountOverflow { phase: ExtensionInvocationPhase },
}

impl fmt::Display for ExtensionSessionCountUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContributionCountOverflow { phase } => write!(
                formatter,
                "extension {} session contribution count overflows u64",
                phase.counter_name()
            ),
        }
    }
}

impl std::error::Error for ExtensionSessionCountUpdateError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestAggregateKey;

    fn tracker() -> ExtensionSessionCountTracker<TestAggregateKey> {
        ExtensionSessionCountTracker::new(TestAggregateKey)
    }

    const fn baseline(attempted: u64, completed: u64) -> ExtensionSessionCountBaseline {
        ExtensionSessionCountBaseline {
            attempted,
            completed,
        }
    }

    const fn baseline_for_phase(
        phase: TrackedPhase,
        contributions: u64,
    ) -> ExtensionSessionCountBaseline {
        match phase {
            TrackedPhase::Attempted => baseline(contributions, 0),
            TrackedPhase::Completed => baseline(0, contributions),
        }
    }

    fn session_id(value: u128) -> SessionId {
        format!("sess_{value:032x}").parse().unwrap()
    }

    fn tracker_at_limit(phase: TrackedPhase) -> ExtensionSessionCountTracker<TestAggregateKey> {
        let mut tracker = tracker();
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            tracker
                .checked_record(
                    baseline_for_phase(phase, value),
                    Some(session_id(u128::from(value))),
                    phase.into(),
                )
                .unwrap();
        }

        tracker
    }

    #[test]
    fn attempted_and_completed_sessions_are_tracked_independently() {
        let mut tracker = tracker();
        let first = session_id(1);
        let second = session_id(2);

        tracker
            .checked_record(
                baseline(0, 0),
                Some(first),
                ExtensionInvocationPhase::Attempted,
            )
            .unwrap();
        tracker
            .checked_record(
                baseline(1, 0),
                Some(second),
                ExtensionInvocationPhase::Completed,
            )
            .unwrap();
        tracker
            .checked_record(
                baseline(1, 1),
                Some(first),
                ExtensionInvocationPhase::Completed,
            )
            .unwrap();

        assert_eq!(
            tracker.snapshot(),
            ExtensionSessionCountSnapshot::Complete {
                identified_sessions: 1,
                identified_sessions_completed: 2,
            }
        );
    }

    #[test]
    fn failed_observations_leave_private_session_state_unchanged() {
        let mut tracker = tracker();
        tracker
            .checked_record(
                baseline(0, 0),
                Some(session_id(1)),
                ExtensionInvocationPhase::Attempted,
            )
            .unwrap();
        let before = tracker.clone();

        let result = tracker.checked_record(
            baseline(u64::MAX, u64::MAX),
            None,
            ExtensionInvocationPhase::Failed,
        );

        assert_eq!(result, Ok(()));
        assert_eq!(tracker, before);
    }

    #[test]
    fn a_missing_phase_session_discards_both_sets_permanently() {
        for phase in [TrackedPhase::Attempted, TrackedPhase::Completed] {
            let mut tracker = tracker();

            tracker
                .checked_record(baseline(0, 0), None, phase.into())
                .unwrap();
            tracker
                .checked_record(
                    baseline_for_phase(phase, 1),
                    Some(session_id(1)),
                    phase.into(),
                )
                .unwrap();

            assert_eq!(
                tracker.snapshot(),
                ExtensionSessionCountSnapshot::Incomplete
            );
        }
    }

    #[test]
    fn either_phase_baseline_mismatch_discards_both_sets() {
        for mismatched_baseline in [baseline(1, 0), baseline(0, 1)] {
            let mut tracker = tracker();

            tracker
                .checked_record(
                    mismatched_baseline,
                    Some(session_id(1)),
                    ExtensionInvocationPhase::Attempted,
                )
                .unwrap();

            assert_eq!(
                tracker.snapshot(),
                ExtensionSessionCountSnapshot::Incomplete
            );
        }
    }

    #[test]
    fn the_256th_distinct_session_is_accepted_in_both_sets() {
        let mut tracker = tracker();
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            tracker
                .checked_record(
                    baseline(value, 0),
                    Some(session_id(u128::from(value))),
                    ExtensionInvocationPhase::Attempted,
                )
                .unwrap();
        }
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            tracker
                .checked_record(
                    baseline(MAX_IDENTIFIED_SESSIONS, value),
                    Some(session_id(u128::from(value))),
                    ExtensionInvocationPhase::Completed,
                )
                .unwrap();
        }

        assert_eq!(
            tracker.snapshot(),
            ExtensionSessionCountSnapshot::Complete {
                identified_sessions: MAX_IDENTIFIED_SESSIONS,
                identified_sessions_completed: MAX_IDENTIFIED_SESSIONS,
            }
        );
    }

    #[test]
    fn a_257th_distinct_session_in_either_set_discards_both_sets() {
        for phase in [TrackedPhase::Attempted, TrackedPhase::Completed] {
            let mut tracker = tracker_at_limit(phase);

            tracker
                .checked_record(
                    baseline_for_phase(phase, MAX_IDENTIFIED_SESSIONS),
                    Some(session_id(u128::from(MAX_IDENTIFIED_SESSIONS))),
                    phase.into(),
                )
                .unwrap();

            assert_eq!(
                tracker.snapshot(),
                ExtensionSessionCountSnapshot::Incomplete
            );
        }
    }

    #[test]
    fn a_known_session_at_the_limit_keeps_its_set_complete() {
        for phase in [TrackedPhase::Attempted, TrackedPhase::Completed] {
            let mut tracker = tracker_at_limit(phase);

            tracker
                .checked_record(
                    baseline_for_phase(phase, MAX_IDENTIFIED_SESSIONS),
                    Some(session_id(0)),
                    phase.into(),
                )
                .unwrap();

            let expected = match phase {
                TrackedPhase::Attempted => ExtensionSessionCountSnapshot::Complete {
                    identified_sessions: MAX_IDENTIFIED_SESSIONS,
                    identified_sessions_completed: 0,
                },
                TrackedPhase::Completed => ExtensionSessionCountSnapshot::Complete {
                    identified_sessions: 0,
                    identified_sessions_completed: MAX_IDENTIFIED_SESSIONS,
                },
            };
            assert_eq!(tracker.snapshot(), expected);
        }
    }

    #[test]
    fn phase_count_overflow_rejects_without_mutation() {
        for phase in [TrackedPhase::Attempted, TrackedPhase::Completed] {
            let mut tracker = tracker();
            let before = tracker.clone();
            let invocation_phase = ExtensionInvocationPhase::from(phase);

            let result =
                tracker.checked_record(baseline_for_phase(phase, u64::MAX), None, invocation_phase);

            assert_eq!(
                result,
                Err(
                    ExtensionSessionCountUpdateError::ContributionCountOverflow {
                        phase: invocation_phase,
                    }
                )
            );
            assert_eq!(tracker, before);
        }
    }

    #[test]
    fn tracker_debug_output_omits_session_identifiers() {
        let mut tracker = tracker();
        let session_id = session_id(1);
        tracker
            .checked_record(
                baseline(0, 0),
                Some(session_id),
                ExtensionInvocationPhase::Attempted,
            )
            .unwrap();

        let debug = format!("{tracker:?}");

        assert!(!debug.contains(&session_id.to_string()));
        assert!(debug.contains("identified_sessions: 1"));
        assert!(debug.contains("identified_sessions_completed: 0"));
    }
}
