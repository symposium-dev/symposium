use super::*;

fn session_id(value: u128) -> SessionId {
    format!("sess_{value:032x}").parse().unwrap()
}

fn values(range: std::ops::Range<u128>) -> Vec<SessionId> {
    range.map(session_id).collect()
}

fn round_trip_hook(pair: &TrackedSessionPair, contributions: u64) -> TrackedSessionPair {
    let encoded = SessionPairRef::from(pair);
    decode_hook(
        encoded.complete,
        encoded
            .first
            .map(|sessions| sessions.iter().copied().collect()),
        encoded
            .second
            .map(|sessions| sessions.iter().copied().collect()),
        contributions,
    )
    .unwrap()
}

#[test]
fn complete_pair_round_trips_without_losing_either_set() {
    let mut pair = TrackedSessionPair::new();
    pair.record_first(session_id(1));
    pair.record_both(session_id(2));

    let decoded = round_trip_hook(&pair, 2);

    // Neither value implements Debug because both can hold session ids.
    assert!(decoded == pair, "decoded session pair differs");
}

#[test]
fn incomplete_pair_round_trips_without_recreating_session_sets() {
    let mut pair = TrackedSessionPair::new();
    pair.invalidate();

    let decoded = round_trip_hook(&pair, u64::MAX);

    assert!(decoded == pair, "decoded session pair differs");
}

#[test]
fn every_other_complete_and_presence_combination_is_rejected() {
    let cases = [
        (false, Some(Vec::new()), None),
        (false, None, Some(Vec::new())),
        (false, Some(Vec::new()), Some(Vec::new())),
        (true, None, None),
        (true, Some(Vec::new()), None),
        (true, None, Some(Vec::new())),
    ];

    for (complete, identified, non_ok) in cases {
        let result = decode_hook(complete, identified, non_ok, 0);

        assert!(matches!(
            result,
            Err(AggregateStateInvariantError::InvalidSessionShape {
                family: AggregateFamily::Hook
            })
        ));
    }
}

#[test]
fn raw_pair_accepts_256_sessions_in_each_set() {
    let pair = decode_extension_invocation(
        true,
        Some(values(0..256)),
        Some(values(256..512)),
        MAX_IDENTIFIED_SESSIONS,
        MAX_IDENTIFIED_SESSIONS,
    )
    .unwrap();

    assert_eq!(
        pair.snapshot(),
        crate::telemetry::state::session_pair::SessionPairSnapshot::Complete {
            first: MAX_IDENTIFIED_SESSIONS,
            second: MAX_IDENTIFIED_SESSIONS,
        }
    );
}

#[test]
fn raw_pair_rejects_more_than_256_sessions_in_either_set() {
    let cases = [
        (Some(values(0..257)), Some(Vec::new())),
        (Some(Vec::new()), Some(values(0..257))),
    ];

    for (identified, non_ok) in cases {
        let result = decode_plugin_hook(true, identified, non_ok, u64::MAX);

        assert!(matches!(
            result,
            Err(AggregateStateInvariantError::TooManySessions {
                family: AggregateFamily::PluginHook
            })
        ));
    }
}

#[test]
fn raw_pair_rejects_duplicate_sessions() {
    let duplicate = session_id(1);
    let result = decode_hook(true, Some(vec![duplicate, duplicate]), Some(Vec::new()), 2);

    assert!(matches!(
        result,
        Err(AggregateStateInvariantError::DuplicateSession {
            family: AggregateFamily::Hook
        })
    ));
}

#[test]
fn hook_and_plugin_non_ok_sessions_must_be_identified() {
    let hook = decode_hook(
        true,
        Some(vec![session_id(1)]),
        Some(vec![session_id(2)]),
        1,
    );
    let plugin = decode_plugin_hook(
        true,
        Some(vec![session_id(1)]),
        Some(vec![session_id(2)]),
        1,
    );

    assert_eq!(
        hook.err(),
        Some(AggregateStateInvariantError::InvalidSessionRelationship {
            family: AggregateFamily::Hook,
        })
    );
    assert_eq!(
        plugin.err(),
        Some(AggregateStateInvariantError::InvalidSessionRelationship {
            family: AggregateFamily::PluginHook,
        })
    );
}

#[test]
fn extension_session_sets_are_independent() {
    let pair = decode_extension_invocation(
        true,
        Some(vec![session_id(1)]),
        Some(vec![session_id(2)]),
        1,
        1,
    )
    .unwrap();

    assert_eq!(
        pair.snapshot(),
        crate::telemetry::state::session_pair::SessionPairSnapshot::Complete {
            first: 1,
            second: 1,
        }
    );
}

#[test]
fn runtime_pair_is_checked_again_before_encoding() {
    let pair = TrackedSessionPair::Complete {
        first: values(0..257).into_iter().collect(),
        second: BTreeSet::new(),
    };

    assert_eq!(
        validate_hook(&pair, u64::MAX),
        Err(AggregateStateInvariantError::TooManySessions {
            family: AggregateFamily::Hook,
        })
    );
}

#[test]
fn hook_contributions_require_only_the_identified_set_to_be_nonempty() {
    let pair = TrackedSessionPair::Complete {
        first: [session_id(1)].into_iter().collect(),
        second: BTreeSet::new(),
    };

    assert_eq!(validate_hook(&pair, 1), Ok(()));
    assert_eq!(validate_plugin_hook(&pair, 1), Ok(()));
}

#[test]
fn positive_hook_contributions_reject_an_empty_identified_set() {
    let pair = TrackedSessionPair::new();

    assert_eq!(
        validate_hook(&pair, 1),
        Err(
            AggregateStateInvariantError::InvalidSessionContributionCount {
                family: AggregateFamily::Hook,
            }
        )
    );
}

#[test]
fn incomplete_pair_accepts_any_contribution_counts() {
    let mut pair = TrackedSessionPair::new();
    pair.invalidate();

    assert_eq!(validate_hook(&pair, u64::MAX), Ok(()));
    assert_eq!(
        validate_extension_invocation(&pair, u64::MAX, u64::MAX),
        Ok(())
    );
}

#[test]
fn extension_contributions_validate_each_matching_session_set() {
    let pair = TrackedSessionPair::Complete {
        first: [session_id(1)].into_iter().collect(),
        second: BTreeSet::new(),
    };

    assert_eq!(
        validate_extension_invocation(&pair, 1, 1),
        Err(
            AggregateStateInvariantError::InvalidSessionContributionCount {
                family: AggregateFamily::ExtensionInvocation,
            }
        )
    );
}

#[test]
fn session_set_cannot_exceed_its_matching_contribution_count() {
    let pair = TrackedSessionPair::Complete {
        first: [session_id(1), session_id(2)].into_iter().collect(),
        second: BTreeSet::new(),
    };

    assert_eq!(
        validate_extension_invocation(&pair, 1, 0),
        Err(
            AggregateStateInvariantError::InvalidSessionContributionCount {
                family: AggregateFamily::ExtensionInvocation,
            }
        )
    );
}
