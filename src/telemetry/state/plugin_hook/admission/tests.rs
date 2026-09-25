use chrono::{TimeZone as _, Utc};

use super::*;
use crate::telemetry::{
    identity::PluginSubject,
    schema::{PluginScope, PublicPluginCoordinate, UtcSecond},
    state::{IDENTIFIER_WINDOW_TEST_STATE, MAX_PUBLIC_ROWS_PER_DAY, TelemetryStateV1},
};

fn state() -> TelemetryStateV1 {
    toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
}

fn recording_at(
    state: &mut TelemetryStateV1,
    day: u32,
    hour: u32,
) -> BoundRecordingObservation<'_> {
    let completed_at =
        UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, day, hour, 2, 11).unwrap());
    let observation = state.observe_recording(completed_at).unwrap();
    state.bind_recording_observation(observation).unwrap()
}

fn public_plugin(name: &str) -> PublicPluginCoordinate {
    serde_json::from_value(serde_json::json!({
        "source": "symposium-recommendations",
        "name": name,
    }))
    .unwrap()
}

fn select<'a>(
    store: &'a mut PluginHookAggregateStore,
    recording: &BoundRecordingObservation<'_>,
    attribution: PluginHookAttribution,
) -> SelectedPluginHookAggregate<'a> {
    store
        .select(
            recording,
            HookAgent::Claude,
            HookSurface::PreToolUse,
            attribution,
        )
        .unwrap()
}

fn fill_public_allowance(
    store: &mut PluginHookAggregateStore,
    recording: &BoundRecordingObservation<'_>,
) {
    for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
        let plugin = public_plugin(&format!("plugin-{index}"));
        let selected = select(store, recording, PluginHookAttribution::Public(plugin));
        assert_eq!(selected.bucket().scope(), PluginScope::Public);
    }
}

#[test]
fn public_admission_keeps_plugin_and_derived_subject_together() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let plugin = public_plugin("example-tools");
    let expected_subject: PluginSubject = recording.identifier_window_scope().derive(&plugin);
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(plugin.clone()),
    );

    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(selected.bucket().plugin(), Some(&plugin));
    assert_eq!(selected.bucket().plugin_subject(), Some(expected_subject));
}

#[test]
fn unnamed_admission_exposes_no_public_identity() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select(&mut store, &recording, PluginHookAttribution::Unnamed);

    assert_eq!(selected.bucket().scope(), PluginScope::Unnamed);
    assert_eq!(selected.bucket().plugin(), None);
    assert_eq!(selected.bucket().plugin_subject(), None);
}

#[test]
fn repeated_public_selection_reuses_event_id_without_spending_again() {
    let plugin = public_plugin("example-tools");
    let mut state = state();
    let first_recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(first_recording.day());
    let first_event_id = select(
        &mut store,
        &first_recording,
        PluginHookAttribution::Public(plugin.clone()),
    )
    .event_id();
    drop(first_recording);
    let second_recording = recording_at(&mut state, 3, 11);

    let second = select(
        &mut store,
        &second_recording,
        PluginHookAttribution::Public(plugin),
    );

    assert_eq!(second.event_id(), first_event_id);
    assert_eq!(second.bucket().scope(), PluginScope::Public);
    assert_eq!(store.admitted_public_rows(), 1);
}

#[test]
fn the_129th_public_plugin_joins_one_overflow_row() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());
    fill_public_allowance(&mut store, &recording);

    let overflow_event_id = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(public_plugin("overflow-a")),
    )
    .event_id();
    let overflow = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(public_plugin("overflow-b")),
    );

    assert_eq!(overflow.event_id(), overflow_event_id);
    assert_eq!(overflow.bucket().scope(), PluginScope::Overflow);
    assert_eq!(overflow.bucket().plugin(), None);
    assert_eq!(overflow.bucket().plugin_subject(), None);
    assert_eq!(store.admitted_public_rows(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn an_existing_public_plugin_remains_selectable_at_capacity() {
    let first_plugin = public_plugin("plugin-0");
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());
    let first_event_id = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(first_plugin.clone()),
    )
    .event_id();
    for index in 1..MAX_PUBLIC_ROWS_PER_DAY {
        let plugin = public_plugin(&format!("plugin-{index}"));
        select(
            &mut store,
            &recording,
            PluginHookAttribution::Public(plugin),
        );
    }

    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(first_plugin),
    );

    assert_eq!(selected.event_id(), first_event_id);
    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(store.admitted_public_rows(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn identifier_reset_does_not_restore_the_daily_allowance() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let day = recording.day();
    let mut store = PluginHookAggregateStore::new(day);
    fill_public_allowance(&mut store, &recording);
    drop(recording);
    state.reset_identifiers(day).unwrap();
    store.reset_identifier_epoch();
    let recording = recording_at(&mut state, 3, 11);

    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(public_plugin("after-reset")),
    );

    assert_eq!(selected.bucket().scope(), PluginScope::Overflow);
    assert_eq!(store.admitted_public_rows(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn clear_restores_the_daily_allowance() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());
    fill_public_allowance(&mut store, &recording);

    store.clear();
    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(public_plugin("after-clear")),
    );

    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(store.admitted_public_rows(), 1);
    assert_eq!(store.len(), 1);
}

#[test]
fn day_rollover_clears_entries_and_restores_the_allowance() {
    let mut state = state();
    let first_recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(first_recording.day());
    fill_public_allowance(&mut store, &first_recording);
    drop(first_recording);
    let next_recording = recording_at(&mut state, 4, 10);

    let selected = select(
        &mut store,
        &next_recording,
        PluginHookAttribution::Public(public_plugin("next-day")),
    );

    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(store.admitted_public_rows(), 1);
    assert_eq!(store.len(), 1);
}

#[test]
fn an_older_observation_is_rejected_without_changing_the_store() {
    let mut telemetry_state = state();
    let older_recording = recording_at(&mut telemetry_state, 3, 10);
    let day = older_recording.day();
    drop(older_recording);
    let newer_recording = recording_at(&mut telemetry_state, 4, 10);
    let mut store = PluginHookAggregateStore::new(newer_recording.day());
    select(&mut store, &newer_recording, PluginHookAttribution::Unnamed);
    let before = store.clone();
    drop(newer_recording);

    let mut older_state = state();
    let older_recording = recording_at(&mut older_state, 3, 11);
    let result = store.select(
        &older_recording,
        HookAgent::Claude,
        HookSurface::PreToolUse,
        PluginHookAttribution::Unnamed,
    );

    assert_eq!(
        result.err(),
        Some(PluginHookAdmissionError::DayBeforeCurrent(
            DayBeforeCurrent {
                current: UtcDay::from_date(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap()),
                observed: day,
            }
        ))
    );
    assert_eq!(store, before);
}
