use std::str::FromStr as _;

use chrono::NaiveDate;

use super::*;
use crate::telemetry::{
    identity::{ExtensionSubject, PluginSubject},
    schema::{RowClassification, TelemetryRow, classify_row, recorded_data_example_row},
};

fn day(day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
}

fn aggregate(kind: &str, event_id: &str) -> AggregateRow {
    let mut value: serde_json::Value =
        serde_json::from_str(recorded_data_example_row(kind)).unwrap();
    value["event_id"] = serde_json::Value::String(event_id.to_owned());
    aggregate_value(&value)
}

fn aggregate_value(value: &serde_json::Value) -> AggregateRow {
    let json = serde_json::to_string(value).unwrap();
    let RowClassification::Supported(TelemetryRow::Aggregate(row)) = classify_row(&json) else {
        panic!("test fixture did not produce a supported aggregate row")
    };
    row
}

fn event_id(value: &str) -> EventId {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).unwrap()
}

fn plugin_identity(
    row: &AggregateRow,
) -> (
    HookAgent,
    HookSurface,
    PublicPluginCoordinate,
    PluginSubject,
) {
    let AggregateRow::PluginHook(row) = row else {
        panic!("expected plugin-hook aggregate")
    };
    let (agent, hook, plugin, subject) = row
        .public_recovery_identity()
        .expect("contract example must be public");
    (agent, hook, plugin.clone(), subject)
}

fn extension_identity(
    row: &AggregateRow,
) -> (
    ExtensionInvocationAgent,
    PublicSkillCoordinate,
    ExtensionSubject,
) {
    let AggregateRow::ExtensionInvocation(row) = row else {
        panic!("expected extension-invocation aggregate")
    };
    let (agent, target, subject) = row
        .public_recovery_identity()
        .expect("contract example must be public");
    (agent, target.clone(), subject)
}

#[test]
fn hook_rows_are_found_by_their_complete_key() {
    let higher = aggregate("hook_metrics", "00000000-0000-4000-8000-000000000002");
    let lower = aggregate("hook_metrics", "00000000-0000-4000-8000-000000000001");
    let AggregateRow::Hook(hook) = &higher else {
        panic!("expected hook aggregate")
    };
    let key = hook.key();
    let mut snapshot = MetricSnapshot::empty(day(3));
    snapshot.insert(higher).unwrap();
    snapshot.insert(lower).unwrap();

    let index = snapshot.recovery_index();

    assert_eq!(index.day(), day(3));
    assert_eq!(
        index.hook_event_id(key),
        Some(event_id("00000000-0000-4000-8000-000000000001"))
    );
}

#[test]
fn an_old_epoch_public_row_counts_but_only_its_subject_is_adoptable() {
    let row = aggregate(
        "plugin_hook_metrics",
        "00000000-0000-4000-8000-000000000001",
    );
    let (agent, hook, plugin, stored_subject) = plugin_identity(&row);
    let current_subject = PluginSubject::from_str("plg_00000000000000000000000000000000").unwrap();
    assert_ne!(current_subject, stored_subject);
    let mut snapshot = MetricSnapshot::empty(day(3));
    snapshot.insert(row).unwrap();

    let index = snapshot.recovery_index();

    assert_eq!(index.plugin_hook_public_rows(), 1);
    assert_eq!(
        index.plugin_hook_event_id(agent, hook, &plugin, current_subject),
        None
    );
    assert_eq!(
        index.plugin_hook_event_id(agent, hook, &plugin, stored_subject),
        Some(event_id("00000000-0000-4000-8000-000000000001"))
    );
}

#[test]
fn duplicate_public_plugin_rows_choose_the_lowest_event_id_and_both_count() {
    let higher = aggregate(
        "plugin_hook_metrics",
        "00000000-0000-4000-8000-000000000002",
    );
    let lower = aggregate(
        "plugin_hook_metrics",
        "00000000-0000-4000-8000-000000000001",
    );
    let (agent, hook, plugin, subject) = plugin_identity(&higher);
    let mut snapshot = MetricSnapshot::empty(day(3));
    snapshot.insert(higher).unwrap();
    snapshot.insert(lower).unwrap();

    let index = snapshot.recovery_index();

    assert_eq!(index.plugin_hook_public_rows(), 2);
    assert_eq!(
        index.plugin_hook_event_id(agent, hook, &plugin, subject),
        Some(event_id("00000000-0000-4000-8000-000000000001"))
    );
}

#[test]
fn extension_rows_have_an_independent_count_and_adoption_lookup() {
    let row = aggregate(
        "extension_invocation_metrics",
        "00000000-0000-4000-8000-000000000003",
    );
    let (agent, target, subject) = extension_identity(&row);
    let mut snapshot = MetricSnapshot::empty(day(3));
    snapshot.insert(row).unwrap();

    let index = snapshot.recovery_index();

    assert_eq!(index.plugin_hook_public_rows(), 0);
    assert_eq!(index.extension_invocation_public_rows(), 1);
    assert_eq!(
        index.extension_invocation_event_id(agent, &target, subject),
        Some(event_id("00000000-0000-4000-8000-000000000003"))
    );
}

#[test]
fn public_row_count_includes_rows_beyond_the_daily_allowance() {
    let template = aggregate(
        "plugin_hook_metrics",
        "00000000-0000-4000-8000-000000000001",
    );
    let mut snapshot = MetricSnapshot::empty(day(3));
    for index in 1_u128..=129 {
        let mut value = serde_json::to_value(&template).unwrap();
        value["event_id"] = serde_json::Value::String(uuid::Uuid::from_u128(index).to_string());
        snapshot.insert(aggregate_value(&value)).unwrap();
    }

    let index = snapshot.recovery_index();

    assert_eq!(index.plugin_hook_public_rows(), 129);
}

#[test]
fn unnamed_and_overflow_rows_are_neither_counted_nor_adoptable() {
    let public_plugin = aggregate(
        "plugin_hook_metrics",
        "00000000-0000-4000-8000-000000000001",
    );
    let (agent, hook, plugin, subject) = plugin_identity(&public_plugin);
    let mut unnamed_value: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("plugin_hook_metrics")).unwrap();
    unnamed_value["event_id"] =
        serde_json::Value::String("00000000-0000-4000-8000-000000000002".to_owned());
    unnamed_value["plugin_scope"] = serde_json::Value::String("unnamed".to_owned());
    unnamed_value.as_object_mut().unwrap().remove("plugin");
    unnamed_value
        .as_object_mut()
        .unwrap()
        .remove("plugin_subject");
    let mut overflow_value = unnamed_value.clone();
    overflow_value["event_id"] =
        serde_json::Value::String("00000000-0000-4000-8000-000000000003".to_owned());
    overflow_value["plugin_scope"] = serde_json::Value::String("overflow".to_owned());
    let mut snapshot = MetricSnapshot::empty(day(3));
    snapshot.insert(aggregate_value(&unnamed_value)).unwrap();
    snapshot.insert(aggregate_value(&overflow_value)).unwrap();

    let index = snapshot.recovery_index();

    assert_eq!(index.plugin_hook_public_rows(), 0);
    assert_eq!(
        index.plugin_hook_event_id(agent, hook, &plugin, subject),
        None
    );
}
