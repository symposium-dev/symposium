use super::*;
use crate::telemetry::{
    identity::SessionId,
    schema::{PluginHookAttribution, PluginHookOutcome},
    state::{IDENTIFIER_WINDOW_TEST_STATE, StageCommit, TelemetryStateV1, recording_observation},
    storage::metrics::MetricSnapshot,
};

const OTHER_KEY: &str = "4343434343434343434343434343434343434343434343434343434343434343";

fn state() -> TelemetryStateV1 {
    toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
}

fn state_with_other_key() -> TelemetryStateV1 {
    let source = IDENTIFIER_WINDOW_TEST_STATE.replace(
        "4242424242424242424242424242424242424242424242424242424242424242",
        OTHER_KEY,
    );
    toml::from_str(&source).unwrap()
}

fn public_plugin(name: &str) -> PublicPluginCoordinate {
    serde_json::from_value(serde_json::json!({
        "source": "symposium-recommendations",
        "name": name,
    }))
    .unwrap()
}

fn session_id(value: u128) -> SessionId {
    format!("sess_{value:032x}").parse().unwrap()
}

fn complete_store(
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
) -> PluginHookAggregateStore {
    let recovery = MetricSnapshot::empty(recording.day()).recovery_index();
    let mut store = PluginHookAggregateStore::new(recording.day());
    let mut stage = store.stage_for_test(&recovery, recording).unwrap();
    stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(public_plugin("example-tools")),
        )
        .unwrap()
        .checked_record_session(0, Some(session_id(1)), PluginHookOutcome::Blocked)
        .unwrap();
    stage
        .select(
            HookAgent::Codex,
            HookSurface::Stop,
            PluginHookAttribution::Unnamed,
        )
        .unwrap()
        .checked_record_session(0, Some(session_id(2)), PluginHookOutcome::Ok)
        .unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
    store
}

fn incomplete_store(
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
) -> PluginHookAggregateStore {
    let recovery = MetricSnapshot::empty(recording.day()).recovery_index();
    let mut store = PluginHookAggregateStore::new(recording.day());
    let mut stage = store.stage_for_test(&recovery, recording).unwrap();
    stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(public_plugin("example-tools")),
        )
        .unwrap()
        .checked_record_session(0, None, PluginHookOutcome::Ok)
        .unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
    store
}

fn encode(
    store: &PluginHookAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<String, AggregateStateInvariantError> {
    let wire = PluginHookStoreRef::try_from((store, scope))?;
    Ok(toml::to_string_pretty(&wire).unwrap())
}

fn decode(
    source: &str,
    scope: &IdentifierWindowScope<'_>,
) -> Result<PluginHookAggregateStore, AggregateStateInvariantError> {
    toml::from_str::<RawPluginHookStore>(source)
        .unwrap()
        .into_runtime(scope)
}

fn encoded_value(
    store: &PluginHookAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> toml::Value {
    let wire = PluginHookStoreRef::try_from((store, scope)).unwrap();
    toml::Value::try_from(&wire).unwrap()
}

fn entry_with_scope_mut<'a>(
    value: &'a mut toml::Value,
    scope: &str,
) -> &'a mut toml::map::Map<String, toml::Value> {
    value["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["scope"].as_str() == Some(scope))
        .unwrap()
        .as_table_mut()
        .unwrap()
}

#[test]
fn complete_plugin_hook_store_round_trips_in_canonical_form() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded plugin-hook store differs from its source"
    );
    assert_eq!(
        encode(&decoded, recording.identifier_window_scope()).unwrap(),
        encoded
    );
    assert!(encoded.find("public-rows-spent").unwrap() < encoded.find("[[entries]]").unwrap());
    assert!(!encoded.contains("plugin-subject"));
}

#[test]
fn permanently_incomplete_plugin_hook_sessions_round_trip_without_arrays() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = incomplete_store(&recording);

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded plugin-hook store differs from its source"
    );
    assert!(encoded.contains("session-counts-complete = false"));
    assert!(!encoded.contains("identified-sessions ="));
    assert!(!encoded.contains("identified-sessions-non-ok ="));
}

#[test]
fn public_row_spend_covers_public_entries_and_may_preserve_older_spend() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);

    for invalid_spend in [0, 129] {
        let mut value = encoded_value(&store, recording.identifier_window_scope());
        value["public-rows-spent"] = toml::Value::Integer(invalid_spend);
        let source = toml::to_string(&value).unwrap();

        assert!(matches!(
            decode(&source, recording.identifier_window_scope()),
            Err(AggregateStateInvariantError::InvalidPublicRowSpend {
                family: AggregateFamily::PluginHook,
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
fn malformed_plugin_bucket_shapes_are_rejected() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);
    let plugin = serde_json::to_value(public_plugin("forged-plugin")).unwrap();
    let plugin = toml::Value::try_from(plugin).unwrap();

    let mut public_without_plugin = encoded_value(&store, recording.identifier_window_scope());
    entry_with_scope_mut(&mut public_without_plugin, "public").remove("plugin");

    let mut unnamed_with_plugin = encoded_value(&store, recording.identifier_window_scope());
    entry_with_scope_mut(&mut unnamed_with_plugin, "unnamed").insert("plugin".to_owned(), plugin);

    for value in [public_without_plugin, unnamed_with_plugin] {
        let source = toml::to_string(&value).unwrap();
        assert!(matches!(
            decode(&source, recording.identifier_window_scope()),
            Err(AggregateStateInvariantError::InvalidBucketShape {
                family: AggregateFamily::PluginHook,
            })
        ));
    }
}

#[test]
fn overflow_bucket_round_trips_without_public_identity() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = incomplete_store(&recording);
    let mut value = encoded_value(&store, recording.identifier_window_scope());
    let public = entry_with_scope_mut(&mut value, "public");
    public.insert(
        "scope".to_owned(),
        toml::Value::String("overflow".to_owned()),
    );
    public.remove("plugin");
    let source = toml::to_string(&value).unwrap();

    let decoded = decode(&source, recording.identifier_window_scope()).unwrap();
    let encoded = encode(&decoded, recording.identifier_window_scope()).unwrap();

    assert!(encoded.contains("scope = \"overflow\""));
    assert!(!encoded.contains("[entries.plugin]"));
}

#[test]
fn duplicate_keys_and_event_ids_are_rejected() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);

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
            family: AggregateFamily::PluginHook,
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
fn plugin_hook_store_decode_runs_session_contribution_validation() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);
    let mut value = encoded_value(&store, recording.identifier_window_scope());
    value["entries"].as_array_mut().unwrap()[0]["contribution-count"] = toml::Value::Integer(0);
    let source = toml::to_string(&value).unwrap();

    assert!(matches!(
        decode(&source, recording.identifier_window_scope()),
        Err(
            AggregateStateInvariantError::InvalidSessionContributionCount {
                family: AggregateFamily::PluginHook,
            }
        )
    ));
}

#[test]
fn plugin_subjects_are_rebuilt_and_never_accepted_from_the_file() {
    let mut first_identity = state();
    let first_recording = recording_observation(&mut first_identity);
    let store = complete_store(&first_recording);
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
        PluginHookStoreRef::try_from((&store, second_recording.identifier_window_scope())),
        Err(AggregateStateInvariantError::EntryIdentityMismatch {
            family: AggregateFamily::PluginHook,
        })
    ));

    let mut value = encoded_value(&store, first_recording.identifier_window_scope());
    entry_with_scope_mut(&mut value, "public").insert(
        "plugin-subject".to_owned(),
        toml::Value::String("plg_00000000000000000000000000000000".to_owned()),
    );
    let source = toml::to_string(&value).unwrap();
    assert!(toml::from_str::<RawPluginHookStore>(&source).is_err());
}
