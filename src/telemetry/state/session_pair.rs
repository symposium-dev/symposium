//! Shared bounded storage for two private aggregate session sets.

use std::collections::BTreeSet;

use crate::telemetry::{identity::SessionId, schema::MAX_IDENTIFIED_SESSIONS};

/// Which sets receive one identified session.
#[derive(Clone, Copy)]
enum Destination {
    First,
    Second,
    Both,
}

/// Two bounded session sets that become permanently incomplete together.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum TrackedSessionPair {
    Complete {
        first: BTreeSet<SessionId>,
        second: BTreeSet<SessionId>,
    },
    Incomplete,
}

impl TrackedSessionPair {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self::Complete {
            first: BTreeSet::new(),
            second: BTreeSet::new(),
        }
    }

    pub(super) fn record_first(&mut self, session_id: SessionId) {
        self.record(session_id, Destination::First);
    }

    pub(super) fn record_second(&mut self, session_id: SessionId) {
        self.record(session_id, Destination::Second);
    }

    pub(super) fn record_both(&mut self, session_id: SessionId) {
        self.record(session_id, Destination::Both);
    }

    pub(super) fn invalidate(&mut self) {
        *self = Self::Incomplete;
    }

    #[must_use]
    pub(super) fn snapshot(&self) -> SessionPairSnapshot {
        match self {
            Self::Complete { first, second } => SessionPairSnapshot::Complete {
                first: set_len(first),
                second: set_len(second),
            },
            Self::Incomplete => SessionPairSnapshot::Incomplete,
        }
    }

    fn record(&mut self, session_id: SessionId, destination: Destination) {
        let Self::Complete { first, second } = self else {
            return;
        };

        let exceeds_limit = match destination {
            Destination::First => would_exceed_limit(first, &session_id),
            Destination::Second => would_exceed_limit(second, &session_id),
            Destination::Both => {
                would_exceed_limit(first, &session_id) || would_exceed_limit(second, &session_id)
            }
        };
        if exceeds_limit {
            *self = Self::Incomplete;
            return;
        }

        match destination {
            Destination::First => {
                first.insert(session_id);
            }
            Destination::Second => {
                second.insert(session_id);
            }
            Destination::Both => {
                first.insert(session_id);
                second.insert(session_id);
            }
        }
    }
}

/// Counts exposed by the shared pair without family-specific names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionPairSnapshot {
    Complete { first: u64, second: u64 },
    Incomplete,
}

fn set_len(sessions: &BTreeSet<SessionId>) -> u64 {
    u64::try_from(sessions.len()).expect("BUG: usize must fit in u64 on supported targets")
}

fn would_exceed_limit(sessions: &BTreeSet<SessionId>, session_id: &SessionId) -> bool {
    !sessions.contains(session_id) && set_len(sessions) >= MAX_IDENTIFIED_SESSIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_id(value: u128) -> SessionId {
        format!("sess_{value:032x}").parse().unwrap()
    }

    fn pair_at_limit() -> TrackedSessionPair {
        let mut pair = TrackedSessionPair::new();
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            pair.record_first(session_id(u128::from(value)));
        }
        pair
    }

    fn pair_with_second_at_limit() -> TrackedSessionPair {
        let mut pair = TrackedSessionPair::new();
        for value in 0..MAX_IDENTIFIED_SESSIONS {
            pair.record_second(session_id(u128::from(value)));
        }
        pair
    }

    #[test]
    fn destinations_update_only_the_selected_sets() {
        let mut pair = TrackedSessionPair::new();

        pair.record_first(session_id(1));
        pair.record_second(session_id(2));
        pair.record_both(session_id(3));

        assert_eq!(
            pair.snapshot(),
            SessionPairSnapshot::Complete {
                first: 2,
                second: 2,
            }
        );
    }

    #[test]
    fn the_257th_distinct_session_discards_both_sets() {
        let mut pair = pair_at_limit();

        pair.record_first(session_id(u128::from(MAX_IDENTIFIED_SESSIONS)));

        assert_eq!(pair.snapshot(), SessionPairSnapshot::Incomplete);
    }

    #[test]
    fn the_257th_distinct_session_in_the_second_set_discards_both_sets() {
        let mut pair = pair_with_second_at_limit();

        pair.record_second(session_id(u128::from(MAX_IDENTIFIED_SESSIONS)));

        assert_eq!(pair.snapshot(), SessionPairSnapshot::Incomplete);
    }

    #[test]
    fn recording_both_checks_the_second_set_limit() {
        let mut pair = pair_with_second_at_limit();

        pair.record_both(session_id(u128::from(MAX_IDENTIFIED_SESSIONS)));

        assert_eq!(pair.snapshot(), SessionPairSnapshot::Incomplete);
    }

    #[test]
    fn a_known_session_at_the_limit_keeps_the_pair_complete() {
        let mut pair = pair_at_limit();

        pair.record_first(session_id(0));

        assert_eq!(
            pair.snapshot(),
            SessionPairSnapshot::Complete {
                first: MAX_IDENTIFIED_SESSIONS,
                second: 0,
            }
        );
    }

    #[test]
    fn invalidation_is_permanent() {
        let mut pair = TrackedSessionPair::new();

        pair.invalidate();
        pair.record_both(session_id(1));

        assert_eq!(pair.snapshot(), SessionPairSnapshot::Incomplete);
    }
}
