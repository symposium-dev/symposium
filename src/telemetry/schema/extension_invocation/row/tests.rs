use super::*;
use crate::telemetry::schema::{
    ExtensionInvocationPhase, MAX_IDENTIFIED_SESSIONS, RowClassification, TelemetryRow,
    classify_row,
    metrics::{ExtensionSessionCountError, SessionSet, SessionSetError},
    recorded_data_example_row,
};

fn extension_invocation_metrics_value() -> serde_json::Value {
    serde_json::from_str(recorded_data_example_row("extension_invocation_metrics")).unwrap()
}

fn unnamed_metrics_value() -> serde_json::Value {
    let mut value = extension_invocation_metrics_value();
    value["target_scope"] = serde_json::json!("unnamed");
    value.as_object_mut().unwrap().remove("target");
    value.as_object_mut().unwrap().remove("extension_subject");
    value["unnamed_reason"] = serde_json::json!("not_indexed");
    value
}

fn overflow_metrics_value() -> serde_json::Value {
    let mut value = extension_invocation_metrics_value();
    value["target_scope"] = serde_json::json!("overflow");
    value.as_object_mut().unwrap().remove("target");
    value.as_object_mut().unwrap().remove("extension_subject");
    value
}

fn validate_value(value: serde_json::Value) -> Result<(), ExtensionInvocationMetricsError> {
    let raw = serde_json::from_value::<RawExtensionInvocationMetricsV1>(value).unwrap();

    validate_extension_invocation_metrics(&raw)
}

fn assert_scope_error(
    value: serde_json::Value,
    scope: ExtensionTargetScope,
    target_present: bool,
    unnamed_reason_present: bool,
    subject_present: bool,
) {
    let result = validate_value(value);

    assert_eq!(
        result,
        Err(
            ExtensionInvocationMetricsError::TargetIdentityDoesNotMatchScope {
                scope,
                target_present,
                unnamed_reason_present,
                subject_present,
            }
        )
    );
}

#[test]
fn extension_invocation_example_round_trips_through_the_classifier() {
    let source = recorded_data_example_row("extension_invocation_metrics");

    let RowClassification::Supported(TelemetryRow::ExtensionInvocationMetrics(row)) =
        classify_row(source)
    else {
        panic!("documented extension-invocation row was not classified as supported");
    };
    let serialized = serde_json::to_string(&row).unwrap();

    assert_eq!(serialized, source);
}

#[test]
fn extension_invocation_rejects_future_versions_unknown_fields_and_missing_fields() {
    let mut future_version = extension_invocation_metrics_value();
    future_version["v"] = serde_json::Value::from(2);
    let future_json = serde_json::to_string(&future_version).unwrap();
    let mut unknown_field = extension_invocation_metrics_value();
    unknown_field["future"] = serde_json::Value::Bool(true);
    let mut missing_field = extension_invocation_metrics_value();
    missing_field.as_object_mut().unwrap().remove("failed");

    assert_eq!(classify_row(&future_json), RowClassification::UnknownSchema);
    for value in [future_version, unknown_field, missing_field] {
        assert!(serde_json::from_value::<ExtensionInvocationMetricsV1>(value).is_err());
    }
}

#[test]
fn invocation_phase_names_match_the_row_counter_fields() {
    let value = extension_invocation_metrics_value();

    for phase in [
        ExtensionInvocationPhase::Attempted,
        ExtensionInvocationPhase::Completed,
        ExtensionInvocationPhase::Failed,
    ] {
        assert!(value.get(phase.counter_name()).is_some());
    }
}

#[test]
fn extension_invocation_rejects_a_row_without_observations() {
    let mut value = extension_invocation_metrics_value();
    value["attempted"] = serde_json::Value::from(0);
    value["completed"] = serde_json::Value::from(0);
    value["failed"] = serde_json::Value::from(0);
    value["identified_sessions"] = serde_json::Value::from(0);
    value["identified_sessions_completed"] = serde_json::Value::from(0);
    let json = serde_json::to_string(&value).unwrap();

    let result = validate_value(value);

    assert_eq!(result, Err(ExtensionInvocationMetricsError::NoObservations));
    assert_eq!(classify_row(&json), RowClassification::Invalid);
}

#[test]
fn public_invocation_metrics_require_a_target_and_subject_without_a_reason() {
    let mut missing_target = extension_invocation_metrics_value();
    missing_target.as_object_mut().unwrap().remove("target");
    let mut missing_subject = extension_invocation_metrics_value();
    missing_subject
        .as_object_mut()
        .unwrap()
        .remove("extension_subject");
    let mut reason_present = extension_invocation_metrics_value();
    reason_present["unnamed_reason"] = serde_json::json!("ambiguous");

    assert_scope_error(
        missing_target,
        ExtensionTargetScope::Public,
        false,
        false,
        true,
    );
    assert_scope_error(
        missing_subject,
        ExtensionTargetScope::Public,
        true,
        false,
        false,
    );
    assert_scope_error(
        reason_present,
        ExtensionTargetScope::Public,
        true,
        true,
        true,
    );
}

#[test]
fn unnamed_invocation_metrics_require_only_an_unnamed_reason() {
    let value = unnamed_metrics_value();

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["unnamed_reason"], "not_indexed");
    assert!(serialized.get("target").is_none());
    assert!(serialized.get("extension_subject").is_none());
}

#[test]
fn unnamed_invocation_metrics_reject_public_identity_or_a_missing_reason() {
    let public = extension_invocation_metrics_value();
    let mut missing_reason = unnamed_metrics_value();
    missing_reason
        .as_object_mut()
        .unwrap()
        .remove("unnamed_reason");
    let mut target_present = unnamed_metrics_value();
    target_present["target"] = public["target"].clone();
    let mut subject_present = unnamed_metrics_value();
    subject_present["extension_subject"] = public["extension_subject"].clone();

    assert_scope_error(
        missing_reason,
        ExtensionTargetScope::Unnamed,
        false,
        false,
        false,
    );
    assert_scope_error(
        target_present,
        ExtensionTargetScope::Unnamed,
        true,
        true,
        false,
    );
    assert_scope_error(
        subject_present,
        ExtensionTargetScope::Unnamed,
        false,
        true,
        true,
    );
}

#[test]
fn overflow_invocation_metrics_omit_target_identity() {
    let value = overflow_metrics_value();

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["target_scope"], "overflow");
    assert!(serialized.get("target").is_none());
    assert!(serialized.get("unnamed_reason").is_none());
    assert!(serialized.get("extension_subject").is_none());
}

#[test]
fn overflow_invocation_metrics_reject_any_target_identity() {
    let public = extension_invocation_metrics_value();
    let mut target_present = overflow_metrics_value();
    target_present["target"] = public["target"].clone();
    let mut reason_present = overflow_metrics_value();
    reason_present["unnamed_reason"] = serde_json::json!("ineligible");
    let mut subject_present = overflow_metrics_value();
    subject_present["extension_subject"] = public["extension_subject"].clone();

    assert_scope_error(
        target_present,
        ExtensionTargetScope::Overflow,
        true,
        false,
        false,
    );
    assert_scope_error(
        reason_present,
        ExtensionTargetScope::Overflow,
        false,
        true,
        false,
    );
    assert_scope_error(
        subject_present,
        ExtensionTargetScope::Overflow,
        false,
        false,
        true,
    );
}

#[test]
fn extension_invocation_rejects_a_non_skill_public_target() {
    let mut value = extension_invocation_metrics_value();
    value["target"]["type"] = serde_json::json!("plugin");
    let json = serde_json::to_string(&value).unwrap();

    let result = serde_json::from_value::<ExtensionInvocationMetricsV1>(value);

    assert!(result.is_err());
    assert_eq!(classify_row(&json), RowClassification::Invalid);
}

#[test]
fn extension_invocation_accepts_independent_phase_and_session_counts() {
    let mut value = extension_invocation_metrics_value();
    value["attempted"] = serde_json::Value::from(1);
    value["completed"] = serde_json::Value::from(2);
    value["failed"] = serde_json::Value::from(2);
    value["identified_sessions"] = serde_json::Value::from(1);
    value["identified_sessions_completed"] = serde_json::Value::from(2);

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["attempted"], 1);
    assert_eq!(serialized["completed"], 2);
    assert_eq!(serialized["failed"], 2);
}

#[test]
fn extension_invocation_accepts_a_failed_only_row() {
    let mut value = extension_invocation_metrics_value();
    value["attempted"] = serde_json::Value::from(0);
    value["completed"] = serde_json::Value::from(0);
    value["failed"] = serde_json::Value::from(1);
    value["identified_sessions"] = serde_json::Value::from(0);
    value["identified_sessions_completed"] = serde_json::Value::from(0);

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["failed"], 1);
    assert_eq!(serialized["identified_sessions"], 0);
    assert_eq!(serialized["identified_sessions_completed"], 0);
}

#[test]
fn extension_invocation_accepts_session_counts_at_the_exact_limit() {
    let mut value = extension_invocation_metrics_value();
    value["attempted"] = serde_json::Value::from(MAX_IDENTIFIED_SESSIONS);
    value["completed"] = serde_json::Value::from(MAX_IDENTIFIED_SESSIONS);
    value["identified_sessions"] = serde_json::Value::from(MAX_IDENTIFIED_SESSIONS);
    value["identified_sessions_completed"] = serde_json::Value::from(MAX_IDENTIFIED_SESSIONS);

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["identified_sessions"], MAX_IDENTIFIED_SESSIONS);
    assert_eq!(
        serialized["identified_sessions_completed"],
        MAX_IDENTIFIED_SESSIONS
    );
}

#[test]
fn extension_invocation_applies_each_session_limit_to_its_phase() {
    let mut attempted = extension_invocation_metrics_value();
    attempted["attempted"] = serde_json::Value::from(1);
    attempted["identified_sessions"] = serde_json::Value::from(2);
    let mut completed = extension_invocation_metrics_value();
    completed["completed"] = serde_json::Value::from(1);
    completed["identified_sessions_completed"] = serde_json::Value::from(2);

    let attempted_result = validate_value(attempted);
    let completed_result = validate_value(completed);

    assert_eq!(
        attempted_result,
        Err(ExtensionInvocationMetricsError::SessionCounts(
            ExtensionSessionCountError::SessionSet(SessionSetError::ExceedsObservations {
                set: SessionSet::Attempted,
                identified: 2,
                observations: 1,
            })
        ))
    );
    assert_eq!(
        completed_result,
        Err(ExtensionInvocationMetricsError::SessionCounts(
            ExtensionSessionCountError::SessionSet(SessionSetError::ExceedsObservations {
                set: SessionSet::Completed,
                identified: 2,
                observations: 1,
            })
        ))
    );
}

#[test]
fn incomplete_extension_session_counts_reject_present_counters() {
    let mut value = extension_invocation_metrics_value();
    value["session_counts_complete"] = serde_json::Value::Bool(false);

    let result = validate_value(value);

    assert_eq!(
        result,
        Err(ExtensionInvocationMetricsError::SessionCounts(
            ExtensionSessionCountError::IncompleteCountsPresent
        ))
    );
}

#[test]
fn incomplete_extension_session_counts_omit_both_counters() {
    let mut value = extension_invocation_metrics_value();
    value["session_counts_complete"] = serde_json::Value::Bool(false);
    value.as_object_mut().unwrap().remove("identified_sessions");
    value
        .as_object_mut()
        .unwrap()
        .remove("identified_sessions_completed");

    let row = serde_json::from_value::<ExtensionInvocationMetricsV1>(value).unwrap();
    let serialized = serde_json::to_value(row).unwrap();

    assert_eq!(serialized["session_counts_complete"], false);
    assert!(serialized.get("identified_sessions").is_none());
    assert!(serialized.get("identified_sessions_completed").is_none());
}
