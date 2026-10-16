use serde_json::json;

use super::*;
use crate::telemetry::{
    identity::SessionId,
    schema::{
        ExtensionInvocationAgent, ExtensionInvocationAttribution, ExtensionInvocationPhase,
        SafeSkillAttribution, UnnamedExtensionReason,
    },
    state::{
        ExtensionInvocationAggregateStore, ExtensionSessionCountBaseline,
        IDENTIFIER_WINDOW_TEST_STATE, StageCommit, TelemetryStateV1, recording_observation,
    },
    storage::metrics::MetricSnapshot,
};

fn state() -> TelemetryStateV1 {
    TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE)
}

fn state_with_other_key() -> TelemetryStateV1 {
    let source = IDENTIFIER_WINDOW_TEST_STATE.replace(
        "4242424242424242424242424242424242424242424242424242424242424242",
        "4343434343434343434343434343434343434343434343434343434343434343",
    );
    TelemetryStateV1::decode_for_test(&source)
}

fn session_id(value: u128) -> SessionId {
    format!("sess_{value:032x}").parse().unwrap()
}

fn attribution(path: &serde_json::Value) -> SafeSkillAttribution {
    serde_json::from_value(json!({
        "target": {
            "type": "skill",
            "source": "symposium-recommendations",
            "name": "example-debugging",
        },
        "path": path,
    }))
    .unwrap()
}

fn recursive_attribution() -> SafeSkillAttribution {
    attribution(&json!([
        {
            "type": "all",
            "children": [
                { "type": "not" },
                {
                    "type": "any",
                    "child": { "type": "opaque", "reason": "private_source" },
                },
            ],
        },
        {
            "type": "package",
            "ecosystem": "cargo",
            "name": "example-runtime",
            "version": "1.2.3",
        },
    ]))
}

fn encode(
    store: &ExtensionInvocationAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<String, AggregateStateInvariantError> {
    let wire = ExtensionInvocationStoreRef::try_from((store, scope))?;
    Ok(toml::to_string_pretty(&wire).unwrap())
}

fn decode(
    source: &str,
    scope: &IdentifierWindowScope<'_>,
) -> Result<ExtensionInvocationAggregateStore, AggregateStateInvariantError> {
    toml::from_str::<RawExtensionInvocationStore>(source)
        .unwrap()
        .into_runtime(scope)
}

fn encoded_value(
    store: &ExtensionInvocationAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> toml::Value {
    let wire = ExtensionInvocationStoreRef::try_from((store, scope)).unwrap();
    toml::Value::try_from(&wire).unwrap()
}

fn first_entry_mut(value: &mut toml::Value) -> &mut toml::map::Map<String, toml::Value> {
    value["entries"].as_array_mut().unwrap()[0]
        .as_table_mut()
        .unwrap()
}

fn record_new_entry(
    store: &mut ExtensionInvocationAggregateStore,
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
    attribution: ExtensionInvocationAttribution,
    phase: ExtensionInvocationPhase,
    session_id: Option<SessionId>,
) {
    record_entry(
        store,
        recording,
        attribution,
        ExtensionSessionCountBaseline {
            attempted: 0,
            completed: 0,
        },
        phase,
        session_id,
    );
}

fn record_entry(
    store: &mut ExtensionInvocationAggregateStore,
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
    attribution: ExtensionInvocationAttribution,
    baseline: ExtensionSessionCountBaseline,
    phase: ExtensionInvocationPhase,
    session_id: Option<SessionId>,
) {
    let recovery = MetricSnapshot::empty(recording.day()).recovery_index();
    let mut stage = store.stage_for_test(&recovery, recording).unwrap();
    stage
        .select(ExtensionInvocationAgent::Claude, attribution)
        .unwrap()
        .session_counts()
        .checked_record(baseline, session_id, phase)
        .unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
}

#[test]
fn complete_extension_store_round_trips_in_canonical_form() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(recursive_attribution()),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );
    record_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(recursive_attribution()),
        ExtensionSessionCountBaseline {
            attempted: 1,
            completed: 0,
        },
        ExtensionInvocationPhase::Completed,
        Some(session_id(2)),
    );
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::NotIndexed),
        ExtensionInvocationPhase::Completed,
        Some(session_id(3)),
    );

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded extension-invocation store differs from its source"
    );
    assert_eq!(
        encode(&decoded, recording.identifier_window_scope()).unwrap(),
        encoded
    );
    assert!(encoded.find("public-rows-spent").unwrap() < encoded.find("[[entries]]").unwrap());
    assert!(!encoded.contains("extension-subject"));
}

#[test]
fn shallow_incomplete_extension_store_round_trips_without_session_arrays() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        None,
    );

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded extension-invocation store differs from its source"
    );
    assert_eq!(
        encode(&decoded, recording.identifier_window_scope()).unwrap(),
        encoded
    );
    assert!(encoded.contains("session-counts-complete = false"));
    assert!(!encoded.contains("identified-sessions ="));
    assert!(!encoded.contains("identified-sessions-completed ="));
}

#[test]
fn public_row_spend_covers_public_entries_and_may_preserve_older_spend() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );

    for invalid_spend in [0, 129] {
        let mut value = encoded_value(&store, recording.identifier_window_scope());
        value["public-rows-spent"] = toml::Value::Integer(invalid_spend);
        let source = toml::to_string(&value).unwrap();

        assert!(matches!(
            decode(&source, recording.identifier_window_scope()),
            Err(AggregateStateInvariantError::InvalidPublicRowSpend {
                family: AggregateFamily::ExtensionInvocation,
            })
        ));
    }

    let mut value = encoded_value(&store, recording.identifier_window_scope());
    value["public-rows-spent"] = toml::Value::Integer(7);
    let source = toml::to_string(&value).unwrap();
    let decoded = decode(&source, recording.identifier_window_scope()).unwrap();
    assert_eq!(decoded.public_rows_spent(), 7);
}

#[test]
fn extension_bucket_fields_have_exact_scope_dependent_shapes() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );
    let base = encoded_value(&store, recording.identifier_window_scope());
    let attribution = base["entries"].as_array().unwrap()[0]["attribution"].clone();

    for scope in ["public", "unnamed", "overflow"] {
        for has_attribution in [false, true] {
            for has_reason in [false, true] {
                let mut value = base.clone();
                let entry = first_entry_mut(&mut value);
                entry.insert("scope".to_owned(), toml::Value::String(scope.to_owned()));
                if has_attribution {
                    entry.insert("attribution".to_owned(), attribution.clone());
                } else {
                    entry.remove("attribution");
                }
                if has_reason {
                    entry.insert(
                        "unnamed-reason".to_owned(),
                        toml::Value::String("not_indexed".to_owned()),
                    );
                } else {
                    entry.remove("unnamed-reason");
                }

                let source = toml::to_string(&value).unwrap();
                let result = decode(&source, recording.identifier_window_scope());
                let valid = matches!(
                    (scope, has_attribution, has_reason),
                    ("public", true, false) | ("unnamed", false, true) | ("overflow", false, false)
                );

                if valid {
                    assert!(result.is_ok(), "valid {scope} bucket was rejected");
                } else {
                    assert!(matches!(
                        result,
                        Err(AggregateStateInvariantError::InvalidBucketShape {
                            family: AggregateFamily::ExtensionInvocation,
                        })
                    ));
                }
            }
        }
    }
}

#[test]
fn duplicate_extension_keys_and_event_ids_are_rejected() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::NotIndexed),
        ExtensionInvocationPhase::Completed,
        Some(session_id(2)),
    );

    let mut duplicate_key = encoded_value(&store, recording.identifier_window_scope());
    let duplicate = duplicate_key["entries"].as_array().unwrap()[0].clone();
    duplicate_key["entries"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let source = toml::to_string(&duplicate_key).unwrap();
    assert!(matches!(
        decode(&source, recording.identifier_window_scope()),
        Err(AggregateStateInvariantError::DuplicateEntry {
            family: AggregateFamily::ExtensionInvocation,
        })
    ));

    let mut duplicate_id = encoded_value(&store, recording.identifier_window_scope());
    let entries = duplicate_id["entries"].as_array_mut().unwrap();
    entries[1]["event-id"] = entries[0]["event-id"].clone();
    let source = toml::to_string(&duplicate_id).unwrap();
    assert!(matches!(
        decode(&source, recording.identifier_window_scope()),
        Err(AggregateStateInvariantError::DuplicateEventId)
    ));
}

#[test]
fn extension_decode_validates_both_session_contribution_counts() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );

    for (field, invalid_count) in [
        ("attempted-contribution-count", 0),
        ("completed-contribution-count", 1),
    ] {
        let mut value = encoded_value(&store, recording.identifier_window_scope());
        first_entry_mut(&mut value).insert(field.to_owned(), toml::Value::Integer(invalid_count));
        let source = toml::to_string(&value).unwrap();

        assert!(matches!(
            decode(&source, recording.identifier_window_scope()),
            Err(
                AggregateStateInvariantError::InvalidSessionContributionCount {
                    family: AggregateFamily::ExtensionInvocation,
                }
            )
        ));
    }
}

#[test]
fn extension_subjects_are_rebuilt_and_never_accepted_from_the_file() {
    let mut first_identity = state();
    let first_recording = recording_observation(&mut first_identity);
    let mut store = ExtensionInvocationAggregateStore::new(first_recording.day());
    record_new_entry(
        &mut store,
        &first_recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );
    let encoded = encode(&store, first_recording.identifier_window_scope()).unwrap();
    let mut second_identity = state_with_other_key();
    let second_recording = recording_observation(&mut second_identity);

    let rebuilt = decode(&encoded, second_recording.identifier_window_scope()).unwrap();
    assert!(rebuilt != store, "a different scope reused stored subjects");
    assert_eq!(
        encode(&rebuilt, second_recording.identifier_window_scope()).unwrap(),
        encoded
    );
    assert!(matches!(
        ExtensionInvocationStoreRef::try_from((&store, second_recording.identifier_window_scope())),
        Err(AggregateStateInvariantError::EntryIdentityMismatch {
            family: AggregateFamily::ExtensionInvocation,
        })
    ));

    let mut value = encoded_value(&store, first_recording.identifier_window_scope());
    first_entry_mut(&mut value).insert(
        "extension-subject".to_owned(),
        toml::Value::String("ext_00000000000000000000000000000000".to_owned()),
    );
    let source = toml::to_string(&value).unwrap();
    assert!(toml::from_str::<RawExtensionInvocationStore>(&source).is_err());
}

#[test]
fn raw_toml_revalidates_recursive_attribution_depth() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    record_new_entry(
        &mut store,
        &recording,
        ExtensionInvocationAttribution::Public(attribution(&json!([{ "type": "not" }]))),
        ExtensionInvocationPhase::Attempted,
        Some(session_id(1)),
    );
    let mut value = encoded_value(&store, recording.identifier_window_scope());
    let mut node = json!({ "type": "not" });
    for _ in 1..9 {
        node = json!({ "type": "any", "child": node });
    }
    let path = toml::Value::try_from(json!([node])).unwrap();
    first_entry_mut(&mut value)["attribution"]
        .as_table_mut()
        .unwrap()
        .insert("path".to_owned(), path);
    let source = toml::to_string(&value).unwrap();

    assert!(toml::from_str::<RawExtensionInvocationStore>(&source).is_err());
}
