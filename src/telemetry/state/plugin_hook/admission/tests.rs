use chrono::{TimeZone as _, Utc};

use super::super::PluginHookAggregateSelectionError;
use super::*;
use crate::telemetry::{
    identity::PluginSubject,
    schema::{
        AggregateRow, EventId, PluginScope, PublicPluginCoordinate, RowClassification,
        TelemetryRow, UtcSecond, classify_row, recorded_data_example_row,
    },
    state::{IDENTIFIER_WINDOW_TEST_STATE, MAX_PUBLIC_ROWS_PER_DAY, TelemetryStateV1},
    storage::metrics::{AggregateRecoveryIndex, MetricSnapshot},
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

fn empty_recovery(day: UtcDay) -> AggregateRecoveryIndex {
    MetricSnapshot::empty(day).recovery_index()
}

fn event_id(value: u128) -> EventId {
    serde_json::from_value(serde_json::Value::String(
        uuid::Uuid::from_u128(value).to_string(),
    ))
    .unwrap()
}

fn plugin_row(
    plugin: &PublicPluginCoordinate,
    subject: PluginSubject,
    event_id: EventId,
) -> AggregateRow {
    let mut value: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("plugin_hook_metrics")).unwrap();
    value["event_id"] = serde_json::to_value(event_id).unwrap();
    value["plugin"] = serde_json::to_value(plugin).unwrap();
    value["plugin_subject"] = serde_json::to_value(subject).unwrap();
    let json = serde_json::to_string(&value).unwrap();
    let RowClassification::Supported(TelemetryRow::Aggregate(row)) = classify_row(&json) else {
        panic!("test fixture did not produce a supported plugin-hook row")
    };
    row
}

fn plugin_recovery(
    day: UtcDay,
    plugin: &PublicPluginCoordinate,
    subject: PluginSubject,
    event_ids: impl IntoIterator<Item = EventId>,
) -> AggregateRecoveryIndex {
    let mut snapshot = MetricSnapshot::empty(day);
    for event_id in event_ids {
        snapshot
            .insert(plugin_row(plugin, subject, event_id))
            .unwrap();
    }
    snapshot.recovery_index()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionSnapshot {
    event_id: EventId,
    bucket: AdmittedPluginBucket,
}

impl SelectionSnapshot {
    const fn event_id(&self) -> EventId {
        self.event_id
    }

    const fn bucket(&self) -> &AdmittedPluginBucket {
        &self.bucket
    }
}

fn select(
    store: &mut PluginHookAggregateStore,
    recording: &BoundRecordingObservation<'_>,
    attribution: PluginHookAttribution,
) -> SelectionSnapshot {
    let recovery = empty_recovery(recording.day());
    select_with_recovery(store, &recovery, recording, attribution)
}

fn select_with_recovery(
    store: &mut PluginHookAggregateStore,
    recovery: &AggregateRecoveryIndex,
    recording: &BoundRecordingObservation<'_>,
    attribution: PluginHookAttribution,
) -> SelectionSnapshot {
    let mut stage = store.stage(recovery, recording).unwrap();
    let selected = stage
        .select(HookAgent::Claude, HookSurface::PreToolUse, attribution)
        .unwrap();
    let snapshot = SelectionSnapshot {
        event_id: selected.event_id(),
        bucket: selected.bucket().clone(),
    };
    assert_eq!(stage.commit(), StageCommit::Applied);
    snapshot
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
    let expected_subject = recording.identifier_window_scope().derive(&plugin);
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(plugin.clone()),
    );

    assert_eq!(
        selected.bucket().public_identity(),
        Some((&plugin, expected_subject))
    );
}

#[test]
fn surviving_public_plugin_is_adopted_before_capacity_overflow() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let plugin = public_plugin("example-tools");
    let subject = recording.identifier_window_scope().derive(&plugin);
    let surviving_event_id = event_id(1);
    let recovery = plugin_recovery(
        recording.day(),
        &plugin,
        subject,
        (1..=MAX_PUBLIC_ROWS_PER_DAY).map(u128::from).map(event_id),
    );
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select_with_recovery(
        &mut store,
        &recovery,
        &recording,
        PluginHookAttribution::Public(plugin),
    );

    assert_eq!(selected.event_id(), surviving_event_id);
    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(store.public_rows_spent(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn old_epoch_public_plugin_counts_but_is_not_adopted() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let plugin = public_plugin("example-tools");
    let old_subject: PluginSubject = "plg_00000000000000000000000000000000".parse().unwrap();
    let old_event_id = event_id(1);
    let recovery = plugin_recovery(recording.day(), &plugin, old_subject, [old_event_id]);
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select_with_recovery(
        &mut store,
        &recovery,
        &recording,
        PluginHookAttribution::Public(plugin),
    );

    assert_ne!(selected.event_id(), old_event_id);
    assert_eq!(selected.bucket().scope(), PluginScope::Public);
    assert_eq!(store.public_rows_spent(), 2);
}

#[test]
fn snapshot_from_another_day_is_rejected_without_changing_the_store() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());
    let before = store.clone();
    let snapshot_day = UtcDay::from_date(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap());
    let recovery = empty_recovery(snapshot_day);

    let result = store.stage(&recovery, &recording);

    assert_eq!(
        result.err(),
        Some(PluginHookAdmissionError::SnapshotDayMismatch {
            snapshot_day,
            observation_day: recording.day(),
        })
    );
    assert_eq!(store, before);
}

#[test]
fn unnamed_admission_exposes_no_public_identity() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(recording.day());

    let selected = select(&mut store, &recording, PluginHookAttribution::Unnamed);

    assert_eq!(selected.bucket().scope(), PluginScope::Unnamed);
    assert_eq!(selected.bucket().public_identity(), None);
}

#[test]
fn repeated_public_selection_reuses_event_id_without_spending_again() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let plugin = public_plugin("example-tools");
    let recovery = empty_recovery(recording.day());
    let mut store = PluginHookAggregateStore::new(recording.day());
    let mut stage = store.stage(&recovery, &recording).unwrap();

    let first_event_id = stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(plugin.clone()),
        )
        .unwrap()
        .event_id();
    let second_event_id = stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(plugin),
        )
        .unwrap()
        .event_id();

    assert_eq!(first_event_id, second_event_id);
    assert_eq!(stage.commit(), StageCommit::Applied);
    assert_eq!(store.public_rows_spent(), 1);
}

#[test]
fn the_129th_public_plugin_joins_one_staged_overflow_row() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let recovery = empty_recovery(recording.day());
    let mut store = PluginHookAggregateStore::new(recording.day());
    let mut stage = store.stage(&recovery, &recording).unwrap();
    for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
        let selected = stage
            .select(
                HookAgent::Claude,
                HookSurface::PreToolUse,
                PluginHookAttribution::Public(public_plugin(&format!("plugin-{index}"))),
            )
            .unwrap();
        assert_eq!(selected.bucket().scope(), PluginScope::Public);
    }

    let first_overflow = stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(public_plugin("overflow-a")),
        )
        .unwrap()
        .event_id();
    let second_overflow = stage
        .select(
            HookAgent::Claude,
            HookSurface::PreToolUse,
            PluginHookAttribution::Public(public_plugin("overflow-b")),
        )
        .unwrap();

    assert_eq!(second_overflow.event_id(), first_overflow);
    assert_eq!(second_overflow.bucket().scope(), PluginScope::Overflow);
    assert_eq!(stage.commit(), StageCommit::Applied);
    assert_eq!(store.public_rows_spent(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn dropping_a_stage_rolls_back_admission_and_allowance_changes() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let recovery = empty_recovery(recording.day());
    let mut store = PluginHookAggregateStore::new(recording.day());
    let before = store.clone();

    {
        let mut stage = store.stage(&recovery, &recording).unwrap();
        stage
            .select(
                HookAgent::Claude,
                HookSurface::PreToolUse,
                PluginHookAttribution::Public(public_plugin("rolled-back")),
            )
            .unwrap();
    }

    assert_eq!(store, before);
}

#[test]
fn committing_a_poisoned_stage_discards_every_edit() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let selected_bucket =
        AdmittedPluginBucket::from_attribution(&recording, PluginHookAttribution::Unnamed);
    let selected_key = PluginHookMetricsKey::new(
        &recording,
        HookAgent::Claude,
        HookSurface::PreToolUse,
        &selected_bucket,
    );
    let other_bucket = AdmittedPluginBucket::from_attribution(
        &recording,
        PluginHookAttribution::Public(public_plugin("another-plugin")),
    );
    let mut store = PluginHookAggregateStore::new(recording.day());
    store.entries.insert(
        selected_key.clone(),
        PluginHookAggregateState::new(&selected_key, other_bucket),
    );
    let before = store.clone();
    let recovery = empty_recovery(recording.day());
    let mut stage = store.stage(&recovery, &recording).unwrap();

    let result = stage.select(
        HookAgent::Claude,
        HookSurface::PreToolUse,
        PluginHookAttribution::Unnamed,
    );

    assert_eq!(
        result.err(),
        Some(PluginHookAdmissionError::PrivateState(
            PluginHookAggregateSelectionError,
        ))
    );
    assert!(stage.is_poisoned());
    assert_eq!(stage.commit(), StageCommit::DiscardedPoisoned);
    assert_eq!(store, before);
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
        select(
            &mut store,
            &recording,
            PluginHookAttribution::Public(public_plugin(&format!("plugin-{index}"))),
        );
    }

    let selected = select(
        &mut store,
        &recording,
        PluginHookAttribution::Public(first_plugin),
    );

    assert_eq!(selected.event_id(), first_event_id);
    assert_eq!(store.public_rows_spent(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn identifier_reset_preserves_the_daily_spend() {
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
    assert_eq!(store.public_rows_spent(), MAX_PUBLIC_ROWS_PER_DAY);
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
    assert_eq!(store.public_rows_spent(), 1);
    assert_eq!(store.len(), 1);
}

#[test]
fn unused_rollover_is_applied_only_when_the_stage_commits() {
    let mut state = state();
    let first_recording = recording_at(&mut state, 3, 10);
    let mut store = PluginHookAggregateStore::new(first_recording.day());
    select(
        &mut store,
        &first_recording,
        PluginHookAttribution::Public(public_plugin("old-day")),
    );
    drop(first_recording);
    let next_recording = recording_at(&mut state, 4, 10);
    let recovery = empty_recovery(next_recording.day());
    let before = store.clone();

    drop(store.stage(&recovery, &next_recording).unwrap());
    assert_eq!(store, before);

    let stage = store.stage(&recovery, &next_recording).unwrap();
    assert_eq!(stage.commit(), StageCommit::Applied);
    assert_eq!(store.public_rows_spent(), 0);
    assert_eq!(store.len(), 0);
}

#[test]
fn committing_an_unused_current_day_stage_is_a_noop() {
    let mut state = state();
    let recording = recording_at(&mut state, 3, 10);
    let recovery = empty_recovery(recording.day());
    let mut store = PluginHookAggregateStore::new(recording.day());
    let before = store.clone();

    let stage = store.stage(&recovery, &recording).unwrap();

    assert_eq!(stage.commit(), StageCommit::Applied);
    assert_eq!(store, before);
}

#[test]
fn an_older_observation_is_rejected_without_changing_the_store() {
    let mut current_state = state();
    let older_recording = recording_at(&mut current_state, 3, 10);
    let older_day = older_recording.day();
    drop(older_recording);
    let newer_recording = recording_at(&mut current_state, 4, 10);
    let mut store = PluginHookAggregateStore::new(newer_recording.day());
    select(&mut store, &newer_recording, PluginHookAttribution::Unnamed);
    let before = store.clone();
    drop(newer_recording);

    let mut older_state = state();
    let older_recording = recording_at(&mut older_state, 3, 11);
    let recovery = empty_recovery(older_day);
    let result = store.stage(&recovery, &older_recording);

    assert_eq!(
        result.err(),
        Some(PluginHookAdmissionError::DayBeforeCurrent(
            DayBeforeCurrent {
                current: UtcDay::from_date(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap()),
                observed: older_day,
            }
        ))
    );
    assert_eq!(store, before);
}
