//! Bounded private session sets for hook aggregate rows.

use std::{collections::BTreeSet, fmt};

use super::plugin_hook::PluginHookMetricsKey;
use crate::telemetry::{
    identity::SessionId,
    schema::{HookMetricsKey, HookOutcome, MAX_IDENTIFIED_SESSIONS, PluginHookOutcome},
};

use super::set_len;

/// One hook observation's contribution to the private session sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HookSessionContribution {
    IdentifiedOk(SessionId),
    IdentifiedNonOk(SessionId),
    Unidentified,
}

impl HookSessionContribution {
    /// Classify one session from the same outcome written into hook metrics.
    #[must_use]
    fn from_hook_outcome(session_id: Option<SessionId>, outcome: HookOutcome) -> Self {
        Self::new(session_id, outcome == HookOutcome::Ok)
    }

    /// Classify one session from the same outcome written into plugin-hook metrics.
    #[must_use]
    fn from_plugin_outcome(session_id: Option<SessionId>, outcome: PluginHookOutcome) -> Self {
        Self::new(session_id, outcome == PluginHookOutcome::Ok)
    }

    fn new(session_id: Option<SessionId>, is_ok: bool) -> Self {
        match (session_id, is_ok) {
            (None, _) => Self::Unidentified,
            (Some(session_id), true) => Self::IdentifiedOk(session_id),
            (Some(session_id), false) => Self::IdentifiedNonOk(session_id),
        }
    }
}

/// Complete or permanently incomplete session sets for one hook aggregate.
#[derive(Clone, PartialEq, Eq)]
enum TrackedHookSessions {
    Complete {
        identified: BTreeSet<SessionId>,
        non_ok: BTreeSet<SessionId>,
    },
    Incomplete,
}

impl TrackedHookSessions {
    fn record(&mut self, contribution: HookSessionContribution) {
        match contribution {
            HookSessionContribution::Unidentified => *self = Self::Incomplete,
            HookSessionContribution::IdentifiedOk(session_id) => {
                self.record_identified(session_id, false);
            }
            HookSessionContribution::IdentifiedNonOk(session_id) => {
                self.record_identified(session_id, true);
            }
        }
    }

    fn record_identified(&mut self, session_id: SessionId, is_non_ok: bool) {
        let Self::Complete { identified, non_ok } = self else {
            return;
        };

        if !identified.contains(&session_id) && set_len(identified) >= MAX_IDENTIFIED_SESSIONS {
            *self = Self::Incomplete;
            return;
        }

        identified.insert(session_id);
        if is_non_ok {
            non_ok.insert(session_id);
        }
    }
}

/// Private session-count state for one daily hook aggregate row.
///
/// The raw and keyed session identifiers never enter the aggregate row. Once
/// an observation lacks an identifier, exceeds the set limit, or disagrees
/// with the snapshot contribution count, this tracker discards both sets and
/// remains incomplete for the rest of its lifetime.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct HookSessionCountTracker<K> {
    key: K,
    contribution_count: u64,
    sessions: TrackedHookSessions,
}

impl<K> fmt::Debug for HookSessionCountTracker<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("HookSessionCountTracker");
        debug.field("contribution_count", &self.contribution_count);

        match &self.sessions {
            TrackedHookSessions::Complete { identified, non_ok } => debug
                .field("session_counts_complete", &true)
                .field("identified_sessions", &identified.len())
                .field("identified_sessions_non_ok", &non_ok.len()),
            TrackedHookSessions::Incomplete => debug.field("session_counts_complete", &false),
        };

        debug.finish()
    }
}

impl<K> HookSessionCountTracker<K> {
    /// Start private session tracking for one aggregate key.
    #[must_use]
    pub(in crate::telemetry) fn new(key: K) -> Self {
        Self {
            key,
            contribution_count: 0,
            sessions: TrackedHookSessions::Complete {
                identified: BTreeSet::new(),
                non_ok: BTreeSet::new(),
            },
        }
    }

    /// Return the aggregate key this private state belongs to.
    #[must_use]
    pub(in crate::telemetry) const fn key(&self) -> &K {
        &self.key
    }

    fn checked_record_contribution(
        &mut self,
        snapshot_contributions: u64,
        contribution: HookSessionContribution,
    ) -> Result<(), HookSessionCountUpdateError> {
        let next_contribution_count = snapshot_contributions
            .checked_add(1)
            .ok_or(HookSessionCountUpdateError::ContributionCountOverflow)?;

        if self.contribution_count != snapshot_contributions {
            self.sessions = TrackedHookSessions::Incomplete;
        }

        self.sessions.record(contribution);
        self.contribution_count = next_contribution_count;
        Ok(())
    }

    /// Return the aggregate fields represented by the current private sets.
    #[must_use]
    pub(in crate::telemetry) fn snapshot(&self) -> HookSessionCountSnapshot {
        match &self.sessions {
            TrackedHookSessions::Complete { identified, non_ok } => {
                HookSessionCountSnapshot::Complete {
                    identified_sessions: set_len(identified),
                    identified_sessions_non_ok: set_len(non_ok),
                }
            }
            TrackedHookSessions::Incomplete => HookSessionCountSnapshot::Incomplete,
        }
    }
}

impl HookSessionCountTracker<HookMetricsKey> {
    /// Add one top-level hook observation without partially changing the tracker.
    ///
    /// A missing identifier or a 257th distinct identifier makes the session
    /// counts incomplete but does not reject the observation. Counter overflow
    /// rejects the complete update and leaves this tracker unchanged.
    /// `snapshot_contributions` is the row's current observation count, or
    /// zero when the row does not exist yet. Requiring it on every call makes
    /// state/snapshot reconciliation part of the update rather than an
    /// optional caller step.
    ///
    /// # Errors
    ///
    /// Returns [`HookSessionCountUpdateError::ContributionCountOverflow`] when
    /// the contribution count cannot be incremented.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        snapshot_contributions: u64,
        session_id: Option<SessionId>,
        outcome: HookOutcome,
    ) -> Result<(), HookSessionCountUpdateError> {
        self.checked_record_contribution(
            snapshot_contributions,
            HookSessionContribution::from_hook_outcome(session_id, outcome),
        )
    }
}

impl HookSessionCountTracker<PluginHookMetricsKey> {
    /// Add one plugin-hook attempt without partially changing the tracker.
    ///
    /// # Errors
    ///
    /// Returns [`HookSessionCountUpdateError::ContributionCountOverflow`] when
    /// the contribution count cannot be incremented.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        snapshot_contributions: u64,
        session_id: Option<SessionId>,
        outcome: PluginHookOutcome,
    ) -> Result<(), HookSessionCountUpdateError> {
        self.checked_record_contribution(
            snapshot_contributions,
            HookSessionContribution::from_plugin_outcome(session_id, outcome),
        )
    }
}

/// Session-count fields supplied to one hook aggregate snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum HookSessionCountSnapshot {
    Complete {
        identified_sessions: u64,
        identified_sessions_non_ok: u64,
    },
    Incomplete,
}

/// A hook session-count update that cannot be represented in private state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum HookSessionCountUpdateError {
    ContributionCountOverflow,
}

impl fmt::Display for HookSessionCountUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContributionCountOverflow => {
                formatter.write_str("hook session contribution count overflows u64")
            }
        }
    }
}

impl std::error::Error for HookSessionCountUpdateError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestAggregateKey;

    fn tracker() -> HookSessionCountTracker<TestAggregateKey> {
        HookSessionCountTracker::new(TestAggregateKey)
    }

    impl HookSessionCountTracker<TestAggregateKey> {
        fn checked_record(
            &mut self,
            snapshot_contributions: u64,
            session_id: Option<SessionId>,
            outcome: HookOutcome,
        ) -> Result<(), HookSessionCountUpdateError> {
            self.checked_record_contribution(
                snapshot_contributions,
                HookSessionContribution::from_hook_outcome(session_id, outcome),
            )
        }
    }

    fn session_id(value: u128) -> SessionId {
        format!("sess_{value:032x}").parse().unwrap()
    }

    fn contribution(
        session_id: Option<SessionId>,
        outcome: HookOutcome,
    ) -> HookSessionContribution {
        HookSessionContribution::from_hook_outcome(session_id, outcome)
    }

    fn tracker_at_limit() -> HookSessionCountTracker<TestAggregateKey> {
        let mut tracker = tracker();
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            tracker
                .checked_record(value, Some(session_id(u128::from(value))), HookOutcome::Ok)
                .unwrap();
        }

        tracker
    }

    #[test]
    fn contribution_classification_uses_the_recorded_hook_outcome() {
        let session_id = session_id(1);

        assert!(matches!(
            contribution(Some(session_id), HookOutcome::Ok),
            HookSessionContribution::IdentifiedOk(_)
        ));
        for outcome in [
            HookOutcome::Blocked,
            HookOutcome::PluginError,
            HookOutcome::InternalError,
        ] {
            assert!(matches!(
                contribution(Some(session_id), outcome),
                HookSessionContribution::IdentifiedNonOk(_)
            ));
        }
        assert!(matches!(
            contribution(None, HookOutcome::InternalError),
            HookSessionContribution::Unidentified
        ));
    }

    #[test]
    fn identified_contributions_count_distinct_and_non_ok_sessions() {
        let mut tracker = tracker();
        let first = session_id(1);
        let second = session_id(2);

        tracker
            .checked_record(0, Some(first), HookOutcome::Ok)
            .unwrap();
        tracker
            .checked_record(1, Some(first), HookOutcome::Blocked)
            .unwrap();
        tracker
            .checked_record(2, Some(second), HookOutcome::PluginError)
            .unwrap();

        assert_eq!(tracker.contribution_count, 3);
        assert_eq!(
            tracker.snapshot(),
            HookSessionCountSnapshot::Complete {
                identified_sessions: 2,
                identified_sessions_non_ok: 2,
            }
        );
    }

    #[test]
    fn an_unidentified_contribution_discards_both_sets_permanently() {
        let mut tracker = tracker();
        tracker
            .checked_record(0, Some(session_id(1)), HookOutcome::Blocked)
            .unwrap();

        tracker.checked_record(1, None, HookOutcome::Ok).unwrap();
        tracker
            .checked_record(2, Some(session_id(2)), HookOutcome::Ok)
            .unwrap();

        assert_eq!(tracker.contribution_count, 3);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
    }

    #[test]
    fn the_256th_distinct_session_is_accepted() {
        let tracker = tracker_at_limit();

        assert_eq!(
            tracker.snapshot(),
            HookSessionCountSnapshot::Complete {
                identified_sessions: MAX_IDENTIFIED_SESSIONS,
                identified_sessions_non_ok: 0,
            }
        );
    }

    #[test]
    fn a_new_session_after_the_limit_discards_both_sets() {
        let mut tracker = tracker_at_limit();

        tracker
            .checked_record(
                MAX_IDENTIFIED_SESSIONS,
                Some(session_id(u128::from(MAX_IDENTIFIED_SESSIONS))),
                HookOutcome::InternalError,
            )
            .unwrap();

        assert_eq!(tracker.contribution_count, MAX_IDENTIFIED_SESSIONS + 1);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
    }

    #[test]
    fn a_known_session_at_the_limit_keeps_counts_complete() {
        let mut tracker = tracker_at_limit();

        tracker
            .checked_record(
                MAX_IDENTIFIED_SESSIONS,
                Some(session_id(0)),
                HookOutcome::Blocked,
            )
            .unwrap();

        assert_eq!(
            tracker.snapshot(),
            HookSessionCountSnapshot::Complete {
                identified_sessions: MAX_IDENTIFIED_SESSIONS,
                identified_sessions_non_ok: 1,
            }
        );
    }

    #[test]
    fn contribution_mismatch_discards_sets_and_uses_the_snapshot_baseline() {
        let mut tracker = tracker();
        tracker
            .checked_record(0, Some(session_id(1)), HookOutcome::Ok)
            .unwrap();

        tracker
            .checked_record(0, Some(session_id(2)), HookOutcome::Ok)
            .unwrap();

        assert_eq!(tracker.contribution_count, 1);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
    }

    #[test]
    fn a_fresh_tracker_for_an_existing_row_starts_incomplete() {
        let mut tracker = tracker();

        tracker
            .checked_record(1, Some(session_id(1)), HookOutcome::Ok)
            .unwrap();

        assert_eq!(tracker.contribution_count, 2);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
    }

    #[test]
    fn matching_snapshot_contributions_preserve_complete_sets_during_update() {
        let mut tracker = tracker();
        tracker
            .checked_record(0, Some(session_id(1)), HookOutcome::Ok)
            .unwrap();

        tracker
            .checked_record(1, Some(session_id(1)), HookOutcome::Ok)
            .unwrap();

        assert_eq!(
            tracker.snapshot(),
            HookSessionCountSnapshot::Complete {
                identified_sessions: 1,
                identified_sessions_non_ok: 0,
            }
        );
    }

    #[test]
    fn contribution_overflow_rejects_the_update_without_mutation() {
        let mut tracker = tracker();
        let before = tracker.clone();

        let result = tracker.checked_record(u64::MAX, None, HookOutcome::InternalError);

        assert_eq!(
            result,
            Err(HookSessionCountUpdateError::ContributionCountOverflow)
        );
        assert_eq!(tracker, before);
    }

    #[test]
    fn tracker_debug_output_omits_session_identifiers() {
        let mut tracker = tracker();
        let session_id = session_id(1);
        tracker
            .checked_record(0, Some(session_id), HookOutcome::Ok)
            .unwrap();

        let debug = format!("{tracker:?}");

        assert!(!debug.contains(&session_id.to_string()));
        assert!(debug.contains("identified_sessions: 1"));
    }
}
