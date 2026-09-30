//! Hook aggregate row and strict read-side validation.

use std::fmt;

use outcome::HookOutcomeCounters;

use super::{
    RowKind, SymposiumVersion,
    agent::HookAgent,
    macros::strict_versioned_row,
    metrics::{LatencyHistogram, SessionCountError, SessionCountInput, validate_session_counts},
};
use crate::telemetry::identity::HookSubject;

mod key;
mod outcome;
mod surface;
mod update;

pub(in crate::telemetry) use key::HookMetricsKey;
pub(in crate::telemetry) use outcome::HookOutcome;
pub(in crate::telemetry) use surface::HookSurface;

strict_versioned_row! {
    /// Version 1 daily aggregate for one agent and hook surface.
    pub(in crate::telemetry) struct HookMetricsV1 {
        symposium: SymposiumVersion,
        agent: HookAgent,
        hook: HookSurface,
        invocations: u64,
        outcomes: HookOutcomeCounters,
        plugins_attempted: u64,
        plugins_completed: u64,
        duration_ms: LatencyHistogram,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions_non_ok: Option<u64>,
        session_counts_complete: bool,
        hook_subject: HookSubject,
    }

    kind: RowKind::HookMetrics,
    raw: RawHookMetricsV1,
    validate: validate_hook_metrics,
}

fn validate_hook_metrics(raw: &RawHookMetricsV1) -> Result<(), HookMetricsError> {
    if raw.invocations == 0 {
        return Err(HookMetricsError::NoInvocations);
    }

    let outcome_total = raw
        .outcomes
        .checked_total()
        .ok_or(HookMetricsError::OutcomeTotalOverflow)?;
    if outcome_total != raw.invocations {
        return Err(HookMetricsError::OutcomeTotalMismatch {
            invocations: raw.invocations,
            outcomes: outcome_total,
        });
    }

    let duration_total = raw
        .duration_ms
        .checked_total()
        .ok_or(HookMetricsError::DurationTotalOverflow)?;
    if duration_total != raw.invocations {
        return Err(HookMetricsError::DurationTotalMismatch {
            invocations: raw.invocations,
            durations: duration_total,
        });
    }

    if raw.plugins_completed > raw.plugins_attempted {
        return Err(HookMetricsError::CompletedPluginsExceedAttempts {
            attempted: raw.plugins_attempted,
            completed: raw.plugins_completed,
        });
    }

    validate_session_counts(SessionCountInput {
        counts_complete: raw.session_counts_complete,
        identified_sessions: raw.identified_sessions,
        identified_sessions_non_ok: raw.identified_sessions_non_ok,
        observations: raw.invocations,
        ok_observations: raw.outcomes.ok,
    })?;

    Ok(())
}

/// Invalid relationship between fields in a hook metrics row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookMetricsError {
    NoInvocations,
    OutcomeTotalOverflow,
    OutcomeTotalMismatch { invocations: u64, outcomes: u64 },
    DurationTotalOverflow,
    DurationTotalMismatch { invocations: u64, durations: u64 },
    CompletedPluginsExceedAttempts { attempted: u64, completed: u64 },
    SessionCounts(SessionCountError),
}

impl From<SessionCountError> for HookMetricsError {
    fn from(error: SessionCountError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for HookMetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInvocations => formatter.write_str("hook metrics row has no invocations"),
            Self::OutcomeTotalOverflow => formatter.write_str("hook outcome total overflows u64"),
            Self::OutcomeTotalMismatch {
                invocations,
                outcomes,
            } => write!(
                formatter,
                "hook outcome total {outcomes} does not match {invocations} invocations"
            ),
            Self::DurationTotalOverflow => formatter.write_str("hook duration total overflows u64"),
            Self::DurationTotalMismatch {
                invocations,
                durations,
            } => write!(
                formatter,
                "hook duration total {durations} does not match {invocations} invocations"
            ),
            Self::CompletedPluginsExceedAttempts {
                attempted,
                completed,
            } => write!(
                formatter,
                "completed plugin hooks {completed} exceed {attempted} attempted plugin hooks"
            ),
            Self::SessionCounts(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for HookMetricsError {}

#[cfg(test)]
mod tests {
    use super::super::{
        RowClassification, TelemetryRow, classify_row,
        metrics::{MAX_IDENTIFIED_SESSIONS, SessionSet, SessionSetError},
        recorded_data_example_row,
    };
    use super::*;

    fn hook_metrics_value() -> serde_json::Value {
        serde_json::from_str(recorded_data_example_row("hook_metrics")).unwrap()
    }

    fn hook_metrics_error(value: serde_json::Value) -> String {
        serde_json::from_value::<HookMetricsV1>(value)
            .unwrap_err()
            .to_string()
    }

    fn validate_hook_metrics_value(value: serde_json::Value) -> Result<(), HookMetricsError> {
        let raw = serde_json::from_value::<RawHookMetricsV1>(value).unwrap();

        validate_hook_metrics(&raw)
    }

    #[test]
    fn hook_metrics_example_round_trips_through_the_classifier() {
        let source = recorded_data_example_row("hook_metrics");

        let RowClassification::Supported(TelemetryRow::HookMetrics(row)) = classify_row(source)
        else {
            panic!("documented hook metrics row was not classified as supported");
        };
        let serialized = serde_json::to_string(&row).unwrap();

        assert_eq!(serialized, source);
    }

    #[test]
    fn hook_metrics_rejects_future_versions_unknown_fields_and_missing_fields() {
        let mut future_version = hook_metrics_value();
        future_version["v"] = serde_json::Value::from(2);
        let future_json = serde_json::to_string(&future_version).unwrap();
        let mut unknown_field = hook_metrics_value();
        unknown_field["future"] = serde_json::Value::Bool(true);
        let mut missing_field = hook_metrics_value();
        missing_field
            .as_object_mut()
            .unwrap()
            .remove("plugins_attempted");

        assert_eq!(classify_row(&future_json), RowClassification::UnknownSchema);
        for value in [future_version, unknown_field, missing_field] {
            assert!(serde_json::from_value::<HookMetricsV1>(value).is_err());
        }
    }

    #[test]
    fn hook_metrics_rejects_an_outcome_total_that_differs_from_invocations() {
        let mut value = hook_metrics_value();
        value["outcomes"]["ok"] = serde_json::Value::from(497);
        let json = serde_json::to_string(&value).unwrap();

        let error = hook_metrics_error(value);

        assert!(error.contains("hook outcome total 499 does not match 500 invocations"));
        assert_eq!(classify_row(&json), RowClassification::Invalid);
    }

    #[test]
    fn hook_metrics_rejects_an_overflowing_outcome_total() {
        let mut value = hook_metrics_value();
        value["outcomes"]["ok"] = serde_json::Value::from(u64::MAX);

        let error = hook_metrics_error(value);

        assert!(error.contains("hook outcome total overflows u64"));
    }

    #[test]
    fn hook_metrics_rejects_a_duration_total_that_differs_from_invocations() {
        let mut value = hook_metrics_value();
        value["duration_ms"]["counts"][7] = serde_json::Value::from(0);

        let error = hook_metrics_error(value);

        assert!(error.contains("hook duration total 499 does not match 500 invocations"));
    }

    #[test]
    fn hook_metrics_rejects_an_overflowing_duration_total() {
        let mut value = hook_metrics_value();
        value["duration_ms"]["counts"][0] = serde_json::Value::from(u64::MAX);

        let error = hook_metrics_error(value);

        assert!(error.contains("hook duration total overflows u64"));
    }

    #[test]
    fn hook_metrics_rejects_more_completed_than_attempted_plugins() {
        let mut value = hook_metrics_value();
        value["plugins_completed"] = serde_json::Value::from(501);

        let error = hook_metrics_error(value);

        assert!(error.contains("completed plugin hooks 501 exceed 500 attempted plugin hooks"));
    }

    #[test]
    fn complete_hook_metrics_requires_both_session_counts() {
        for missing in ["identified_sessions", "identified_sessions_non_ok"] {
            let mut value = hook_metrics_value();
            value.as_object_mut().unwrap().remove(missing);

            let error = hook_metrics_error(value);

            assert!(error.contains("require both identified session counters"));
        }
    }

    #[test]
    fn incomplete_hook_metrics_omits_both_session_counts() {
        let mut value = hook_metrics_value();
        value["session_counts_complete"] = serde_json::Value::Bool(false);
        value.as_object_mut().unwrap().remove("identified_sessions");
        value
            .as_object_mut()
            .unwrap()
            .remove("identified_sessions_non_ok");

        let row = serde_json::from_value::<HookMetricsV1>(value).unwrap();
        let serialized = serde_json::to_value(row).unwrap();

        assert_eq!(serialized["session_counts_complete"], false);
        assert!(serialized.get("identified_sessions").is_none());
        assert!(serialized.get("identified_sessions_non_ok").is_none());
    }

    #[test]
    fn incomplete_hook_metrics_rejects_either_session_count() {
        for present in ["identified_sessions", "identified_sessions_non_ok"] {
            let mut value = hook_metrics_value();
            value["session_counts_complete"] = serde_json::Value::Bool(false);
            value.as_object_mut().unwrap().remove("identified_sessions");
            value
                .as_object_mut()
                .unwrap()
                .remove("identified_sessions_non_ok");
            value[present] = serde_json::Value::from(1);

            let error = hook_metrics_error(value);

            assert!(error.contains("must omit both identified session counters"));
        }
    }

    #[test]
    fn hook_metrics_rejects_more_non_ok_than_identified_sessions() {
        let mut value = hook_metrics_value();
        value["identified_sessions"] = serde_json::Value::from(1);
        value["identified_sessions_non_ok"] = serde_json::Value::from(2);

        let error = hook_metrics_error(value);

        assert!(error.contains("non-ok identified sessions 2 exceed 1 identified sessions"));
    }

    #[test]
    fn hook_metrics_rejects_more_than_256_identified_sessions() {
        let mut value = hook_metrics_value();
        value["identified_sessions"] = serde_json::Value::from(257);

        let result = validate_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(HookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::ExceedsLimit {
                    set: SessionSet::Identified,
                    identified: 257,
                    maximum: MAX_IDENTIFIED_SESSIONS,
                })
            ))
        );
    }

    #[test]
    fn hook_metrics_rejects_more_identified_sessions_than_invocations() {
        let mut value = hook_metrics_value();
        value["invocations"] = serde_json::Value::from(1);
        value["outcomes"] = serde_json::json!({
            "ok": 0,
            "blocked": 1,
            "plugin_error": 0,
            "internal_error": 0,
        });
        value["duration_ms"]["counts"] = serde_json::json!([1, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["identified_sessions"] = serde_json::Value::from(2);
        value["identified_sessions_non_ok"] = serde_json::Value::from(1);

        let result = validate_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(HookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::ExceedsObservations {
                    set: SessionSet::Identified,
                    identified: 2,
                    observations: 1,
                })
            ))
        );
    }

    #[test]
    fn hook_metrics_rejects_more_non_ok_sessions_than_non_ok_invocations() {
        let mut value = hook_metrics_value();
        value["identified_sessions"] = serde_json::Value::from(3);
        value["identified_sessions_non_ok"] = serde_json::Value::from(3);

        let result = validate_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(HookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::ExceedsObservations {
                    set: SessionSet::NonOk,
                    identified: 3,
                    observations: 2,
                })
            ))
        );
    }

    #[test]
    fn hook_metrics_rejects_an_empty_aggregate() {
        let mut value = hook_metrics_value();
        value["invocations"] = serde_json::Value::from(0);
        value["outcomes"] = serde_json::json!({
            "ok": 0,
            "blocked": 0,
            "plugin_error": 0,
            "internal_error": 0,
        });
        value["plugins_attempted"] = serde_json::Value::from(0);
        value["plugins_completed"] = serde_json::Value::from(0);
        value["duration_ms"]["counts"] = serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["session_counts_complete"] = serde_json::Value::Bool(false);
        value.as_object_mut().unwrap().remove("identified_sessions");
        value
            .as_object_mut()
            .unwrap()
            .remove("identified_sessions_non_ok");

        let error = hook_metrics_error(value);

        assert!(error.contains("hook metrics row has no invocations"));
    }

    #[test]
    fn complete_hook_metrics_requires_at_least_one_identified_session() {
        let mut value = hook_metrics_value();
        value["identified_sessions"] = serde_json::Value::from(0);
        value["identified_sessions_non_ok"] = serde_json::Value::from(0);

        let result = validate_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(HookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::NoSessions {
                    set: SessionSet::Identified,
                    observations: 500,
                })
            ))
        );
    }

    #[test]
    fn non_ok_invocations_require_at_least_one_non_ok_session() {
        let mut value = hook_metrics_value();
        value["identified_sessions_non_ok"] = serde_json::Value::from(0);

        let result = validate_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(HookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::NoSessions {
                    set: SessionSet::NonOk,
                    observations: 2,
                })
            ))
        );
    }

    #[test]
    fn hook_metrics_rejects_more_all_ok_sessions_than_ok_invocations() {
        let mut value = hook_metrics_value();
        value["invocations"] = serde_json::Value::from(3);
        value["outcomes"] = serde_json::json!({
            "ok": 1,
            "blocked": 2,
            "plugin_error": 0,
            "internal_error": 0,
        });
        value["duration_ms"]["counts"] = serde_json::json!([3, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["identified_sessions"] = serde_json::Value::from(3);
        value["identified_sessions_non_ok"] = serde_json::Value::from(1);

        let error = hook_metrics_error(value);

        assert!(error.contains("all-ok identified sessions 2 exceed 1 ok observations"));
    }

    #[test]
    fn hook_metrics_accepts_exact_session_count_limits() {
        let mut value = hook_metrics_value();
        value["invocations"] = serde_json::Value::from(256);
        value["outcomes"] = serde_json::json!({
            "ok": 254,
            "blocked": 2,
            "plugin_error": 0,
            "internal_error": 0,
        });
        value["duration_ms"]["counts"] = serde_json::json!([256, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["identified_sessions"] = serde_json::Value::from(256);
        value["identified_sessions_non_ok"] = serde_json::Value::from(2);
        let json = serde_json::to_string(&value).unwrap();

        let classification = classify_row(&json);

        assert!(matches!(
            classification,
            RowClassification::Supported(TelemetryRow::HookMetrics(_))
        ));
    }
}
