use std::{collections::BTreeMap, time::Duration};

use chrono::NaiveDate;

use super::*;
use crate::telemetry::{
    schema::{
        ExtensionInvocationAttribution, ExtensionInvocationPhase, HookAgent, HookOutcome,
        HookSurface, PluginHookAttempt, PluginHookAttribution, UnnamedExtensionReason,
    },
    state::{
        ExtensionInvocationAggregateStore, IDENTIFIER_WINDOW_TEST_STATE, PluginHookAggregateStore,
        TelemetryStateV1,
        aggregate::{
            AggregateState, recording::ExtensionInvocationObservation,
            recording::HookInvocationMetricObservation, recording::PluginHookInvocationObservation,
        },
        extension_invocation::ExtensionInvocationAggregateState,
        hook::HookAggregateStore,
        recording_observation,
    },
    storage::metrics::MetricSnapshot,
};

fn state() -> TelemetryStateV1 {
    TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE)
}

fn day(day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
}

fn context<'scope>(
    recording: &'scope crate::telemetry::state::BoundRecordingObservation<'_>,
) -> AggregatePersistenceContext<'scope, 'scope> {
    AggregatePersistenceContext::new(recording.identifier_window_scope(), recording.day())
}

fn populated_aggregates(
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
) -> AggregateState {
    let mut aggregates = AggregateState::new(recording.day());
    let observation = HookInvocationMetricObservation {
        agent: HookAgent::Claude,
        hook: HookSurface::PreToolUse,
        outcome: HookOutcome::Ok,
        duration: Duration::from_millis(7),
        vendor_session_id: None,
        plugin_attempts: vec![PluginHookInvocationObservation {
            attribution: PluginHookAttribution::Unnamed,
            terminal: Some(PluginHookAttempt::NotExecuted {
                prepare_duration: Duration::from_millis(2),
            }),
        }],
        extension: Some(ExtensionInvocationObservation {
            attribution: ExtensionInvocationAttribution::Unnamed(
                UnnamedExtensionReason::AttributionUnavailable,
            ),
            phase: ExtensionInvocationPhase::Attempted,
        }),
    };

    let _staged = aggregates
        .stage_hook_invocation(
            recording,
            MetricSnapshot::empty(recording.day()),
            observation,
        )
        .unwrap();
    aggregates
}

#[test]
fn complete_aggregate_state_round_trips_canonically() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let aggregates = populated_aggregates(&recording);

    let wire = AggregateStateRef::try_from((&aggregates, context(&recording))).unwrap();
    let encoded = toml::to_string_pretty(&wire).unwrap();
    let decoded = toml::from_str::<RawAggregateState>(&encoded)
        .unwrap()
        .into_runtime(context(&recording))
        .unwrap();
    let reencoded = toml::to_string_pretty(
        &AggregateStateRef::try_from((&decoded, context(&recording))).unwrap(),
    )
    .unwrap();

    assert!(decoded == aggregates, "decoded aggregate state differs");
    assert_eq!(reencoded, encoded);
}

#[test]
fn every_store_day_must_match_the_identity_high_water_day() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let cases = [
        {
            let mut aggregates = AggregateState::new(recording.day());
            aggregates.hook = HookAggregateStore::new(day(4));
            (AggregateFamily::Hook, aggregates)
        },
        {
            let mut aggregates = AggregateState::new(recording.day());
            aggregates.plugin_hook = PluginHookAggregateStore::new(day(4));
            (AggregateFamily::PluginHook, aggregates)
        },
        {
            let mut aggregates = AggregateState::new(recording.day());
            aggregates.extension_invocation = ExtensionInvocationAggregateStore::new(day(4));
            (AggregateFamily::ExtensionInvocation, aggregates)
        },
    ];

    for (expected, aggregates) in cases {
        let result = AggregateStateRef::try_from((&aggregates, context(&recording)));
        assert!(
            matches!(
                result,
                Err(AggregateStateInvariantError::StoreDayMismatch { family })
                    if family == expected
            ),
            "wrong mismatch family for {expected}"
        );
    }

    let valid = AggregateState::new(recording.day());
    let encoded = toml::to_string_pretty(
        &AggregateStateRef::try_from((&valid, context(&recording))).unwrap(),
    )
    .unwrap();
    let decode_result = toml::from_str::<RawAggregateState>(&encoded)
        .unwrap()
        .into_runtime(AggregatePersistenceContext::new(
            recording.identifier_window_scope(),
            day(4),
        ));

    assert!(matches!(
        decode_result,
        Err(AggregateStateInvariantError::StoreDayMismatch {
            family: AggregateFamily::Hook,
        })
    ));
}

#[test]
fn event_ids_must_be_unique_across_persisted_families() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut aggregates = populated_aggregates(&recording);
    let (_, plugin_entry) = aggregates.plugin_hook.persistence_entries().next().unwrap();
    let (plugin_event_id, _, _) = plugin_entry.persistence_parts();
    let (key, extension_entry) = aggregates
        .extension_invocation
        .persistence_entries()
        .next()
        .unwrap();
    let key = key.clone();
    let (extension_event_id, bucket, tracker) = extension_entry.persistence_parts();
    let bucket = bucket.clone();
    let (attempted, completed, sessions) = tracker.persistence_parts();
    let sessions = sessions.clone();
    let encoded = toml::to_string_pretty(
        &AggregateStateRef::try_from((&aggregates, context(&recording))).unwrap(),
    )
    .unwrap();
    let replacement = ExtensionInvocationAggregateState::from_persisted(
        &key,
        plugin_event_id,
        bucket,
        attempted,
        completed,
        sessions,
    );
    aggregates.extension_invocation = ExtensionInvocationAggregateStore::from_persisted(
        recording.day(),
        aggregates.extension_invocation.public_rows_spent(),
        BTreeMap::from([(key, replacement)]),
    );

    let encode_result = AggregateStateRef::try_from((&aggregates, context(&recording)));

    assert!(matches!(
        encode_result,
        Err(AggregateStateInvariantError::DuplicateEventId)
    ));

    let duplicate = encoded.replacen(
        &format!("event-id = \"{extension_event_id}\""),
        &format!("event-id = \"{plugin_event_id}\""),
        1,
    );
    assert_ne!(duplicate, encoded, "fixture event id was not replaced");
    let decode_result = toml::from_str::<RawAggregateState>(&duplicate)
        .unwrap()
        .into_runtime(context(&recording));

    assert!(matches!(
        decode_result,
        Err(AggregateStateInvariantError::DuplicateEventId)
    ));
}
