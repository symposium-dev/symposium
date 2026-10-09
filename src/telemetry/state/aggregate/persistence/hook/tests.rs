use super::*;
use crate::telemetry::{
    identity::SessionId,
    schema::HookOutcome,
    state::{
        HookAggregateStore, IDENTIFIER_WINDOW_TEST_STATE, StageCommit, TelemetryStateV1,
        recording_observation,
    },
};

const OTHER_KEY: &str = "4343434343434343434343434343434343434343434343434343434343434343";

fn state() -> TelemetryStateV1 {
    TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE)
}

fn state_with_other_key() -> TelemetryStateV1 {
    let source = IDENTIFIER_WINDOW_TEST_STATE.replace(
        "4242424242424242424242424242424242424242424242424242424242424242",
        OTHER_KEY,
    );
    TelemetryStateV1::decode_for_test(&source)
}

fn session_id(value: u128) -> SessionId {
    format!("sess_{value:032x}").parse().unwrap()
}

fn complete_store(
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
) -> HookAggregateStore {
    let mut store = HookAggregateStore::new(recording.day());
    let mut stage = store.stage_for_test(recording).unwrap();
    stage
        .select(HookAgent::Claude, HookSurface::PreToolUse)
        .unwrap()
        .checked_record(0, Some(session_id(1)), HookOutcome::Blocked)
        .unwrap();
    stage
        .select(HookAgent::Codex, HookSurface::Stop)
        .unwrap()
        .checked_record(0, Some(session_id(2)), HookOutcome::Ok)
        .unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
    store
}

fn incomplete_store(
    recording: &crate::telemetry::state::BoundRecordingObservation<'_>,
) -> HookAggregateStore {
    let mut store = HookAggregateStore::new(recording.day());
    let mut stage = store.stage_for_test(recording).unwrap();
    stage
        .select(HookAgent::Claude, HookSurface::PreToolUse)
        .unwrap()
        .checked_record(0, None, HookOutcome::Ok)
        .unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
    store
}

fn encode(
    store: &HookAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<String, AggregateStateInvariantError> {
    let wire = HookStoreRef::try_from((store, scope))?;
    Ok(toml::to_string_pretty(&wire).unwrap())
}

fn decode(
    source: &str,
    scope: &IdentifierWindowScope<'_>,
) -> Result<HookAggregateStore, AggregateStateInvariantError> {
    toml::from_str::<RawHookStore>(source)
        .unwrap()
        .into_runtime(scope)
}

#[test]
fn complete_hook_store_round_trips_in_canonical_form() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded hook store differs from its source"
    );
    assert_eq!(
        encode(&decoded, recording.identifier_window_scope()).unwrap(),
        encoded
    );
    assert!(encoded.contains("contribution-count = 1"));
    assert!(!encoded.contains("hook-subject"));
}

#[test]
fn permanently_incomplete_hook_sessions_round_trip_without_arrays() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = incomplete_store(&recording);

    let encoded = encode(&store, recording.identifier_window_scope()).unwrap();
    let decoded = decode(&encoded, recording.identifier_window_scope()).unwrap();

    assert!(
        decoded == store,
        "decoded hook store differs from its source"
    );
    assert!(encoded.contains("session-counts-complete = false"));
    assert!(!encoded.contains("identified-sessions ="));
    assert!(!encoded.contains("identified-sessions-non-ok ="));
}

#[test]
fn duplicate_reconstructed_hook_keys_are_rejected() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);
    let wire = HookStoreRef::try_from((&store, recording.identifier_window_scope())).unwrap();
    let mut value = toml::Value::try_from(&wire).unwrap();
    let entries = value["entries"].as_array_mut().unwrap();
    entries.push(entries[0].clone());
    let source = toml::to_string(&value).unwrap();

    let result = decode(&source, recording.identifier_window_scope());

    assert!(matches!(
        result,
        Err(AggregateStateInvariantError::DuplicateEntry {
            family: AggregateFamily::Hook,
        })
    ));
}

#[test]
fn hook_store_decode_runs_session_contribution_validation() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);
    let wire = HookStoreRef::try_from((&store, recording.identifier_window_scope())).unwrap();
    let mut value = toml::Value::try_from(&wire).unwrap();
    value["entries"].as_array_mut().unwrap()[0]["contribution-count"] = toml::Value::Integer(0);
    let source = toml::to_string(&value).unwrap();

    let result = decode(&source, recording.identifier_window_scope());

    assert!(matches!(
        result,
        Err(
            AggregateStateInvariantError::InvalidSessionContributionCount {
                family: AggregateFamily::Hook,
            }
        )
    ));
}

#[test]
fn hook_store_encoding_rejects_keys_from_another_identity_scope() {
    let mut first_identity = state();
    let first_recording = recording_observation(&mut first_identity);
    let store = complete_store(&first_recording);
    let mut second_identity = state_with_other_key();
    let second_recording = recording_observation(&mut second_identity);

    let result = HookStoreRef::try_from((&store, second_recording.identifier_window_scope()));

    assert!(matches!(
        result,
        Err(AggregateStateInvariantError::EntryIdentityMismatch {
            family: AggregateFamily::Hook,
        })
    ));
}

#[test]
fn hook_store_decode_rebuilds_subjects_from_the_supplied_scope() {
    let mut first_identity = state();
    let first_recording = recording_observation(&mut first_identity);
    let store = complete_store(&first_recording);
    let encoded = encode(&store, first_recording.identifier_window_scope()).unwrap();
    let mut second_identity = state_with_other_key();
    let second_recording = recording_observation(&mut second_identity);

    let rebuilt = decode(&encoded, second_recording.identifier_window_scope()).unwrap();

    assert!(
        rebuilt != store,
        "a different identity scope reused stored subjects"
    );
    assert_eq!(
        encode(&rebuilt, second_recording.identifier_window_scope()).unwrap(),
        encoded
    );
}

#[test]
fn persisted_hook_subject_is_rejected_instead_of_trusted() {
    let mut identity = state();
    let recording = recording_observation(&mut identity);
    let store = complete_store(&recording);
    let wire = HookStoreRef::try_from((&store, recording.identifier_window_scope())).unwrap();
    let mut value = toml::Value::try_from(&wire).unwrap();
    value["entries"].as_array_mut().unwrap()[0]
        .as_table_mut()
        .unwrap()
        .insert(
            "hook-subject".to_owned(),
            toml::Value::String("hok_00000000000000000000000000000000".to_owned()),
        );
    let source = toml::to_string(&value).unwrap();

    let result = toml::from_str::<RawHookStore>(&source);

    assert!(result.is_err());
}
