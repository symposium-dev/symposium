//! Plugin-hook aggregate row and strict read-side validation.

use std::fmt;

use outcome::PluginHookOutcomeCounters;

use super::{
    RowKind, SymposiumVersion,
    agent::HookAgent,
    hook::HookSurface,
    macros::strict_versioned_row,
    metrics::{LatencyHistogram, SessionCountError, SessionCountInput, validate_session_counts},
};
use crate::telemetry::identity::PluginSubject;

mod bucket;
mod outcome;
mod update;

pub(in crate::telemetry) use bucket::{PluginHookAttribution, PluginScope, PublicPluginCoordinate};
pub(in crate::telemetry) use outcome::PluginHookOutcome;
strict_versioned_row! {
    /// Version 1 daily aggregate for one plugin, agent, and hook surface.
    pub(in crate::telemetry) struct PluginHookMetricsV1 {
        symposium: SymposiumVersion,
        agent: HookAgent,
        hook: HookSurface,
        plugin_scope: PluginScope,
        #[serde(skip_serializing_if = "Option::is_none")]
        plugin: Option<PublicPluginCoordinate>,
        attempts: u64,
        executions: u64,
        outcomes: PluginHookOutcomeCounters,
        prepare_ms: LatencyHistogram,
        execute_ms: LatencyHistogram,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions_non_ok: Option<u64>,
        session_counts_complete: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        plugin_subject: Option<PluginSubject>,
    }

    kind: RowKind::PluginHookMetrics,
    raw: RawPluginHookMetricsV1,
    validate: validate_plugin_hook_metrics,
}

fn validate_plugin_hook_metrics(
    raw: &RawPluginHookMetricsV1,
) -> Result<(), PluginHookMetricsError> {
    if raw.attempts == 0 {
        return Err(PluginHookMetricsError::NoAttempts);
    }

    let outcome_total = raw
        .outcomes
        .checked_total()
        .ok_or(PluginHookMetricsError::OutcomeTotalOverflow)?;
    if outcome_total != raw.attempts {
        return Err(PluginHookMetricsError::OutcomeTotalMismatch {
            attempts: raw.attempts,
            outcomes: outcome_total,
        });
    }

    let prepare_total = raw
        .prepare_ms
        .checked_total()
        .ok_or(PluginHookMetricsError::PrepareDurationTotalOverflow)?;
    if prepare_total != raw.attempts {
        return Err(PluginHookMetricsError::PrepareDurationTotalMismatch {
            attempts: raw.attempts,
            durations: prepare_total,
        });
    }

    if raw.executions > raw.attempts {
        return Err(PluginHookMetricsError::ExecutionsExceedAttempts {
            attempts: raw.attempts,
            executions: raw.executions,
        });
    }

    // The subset check above proves that this subtraction cannot underflow.
    // Every attempt that stopped before child execution has an error outcome.
    let non_executed = raw.attempts - raw.executions;
    if non_executed > raw.outcomes.error {
        return Err(
            PluginHookMetricsError::NonExecutedAttemptsExceedErrorOutcomes {
                non_executed,
                errors: raw.outcomes.error,
            },
        );
    }

    let execute_total = raw
        .execute_ms
        .checked_total()
        .ok_or(PluginHookMetricsError::ExecuteDurationTotalOverflow)?;
    if execute_total != raw.executions {
        return Err(PluginHookMetricsError::ExecuteDurationTotalMismatch {
            executions: raw.executions,
            durations: execute_total,
        });
    }

    let plugin_present = raw.plugin.is_some();
    let subject_present = raw.plugin_subject.is_some();
    let identity_matches_scope = match raw.plugin_scope {
        PluginScope::Public => plugin_present && subject_present,
        PluginScope::Unnamed | PluginScope::Overflow => !plugin_present && !subject_present,
    };
    if !identity_matches_scope {
        return Err(PluginHookMetricsError::PluginIdentityDoesNotMatchScope {
            scope: raw.plugin_scope,
            plugin_present,
            subject_present,
        });
    }

    validate_session_counts(SessionCountInput {
        counts_complete: raw.session_counts_complete,
        identified_sessions: raw.identified_sessions,
        identified_sessions_non_ok: raw.identified_sessions_non_ok,
        observations: raw.attempts,
        ok_observations: raw.outcomes.ok,
    })?;

    Ok(())
}

/// Invalid relationship between fields in a plugin-hook metrics row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginHookMetricsError {
    NoAttempts,
    OutcomeTotalOverflow,
    OutcomeTotalMismatch {
        attempts: u64,
        outcomes: u64,
    },
    PrepareDurationTotalOverflow,
    PrepareDurationTotalMismatch {
        attempts: u64,
        durations: u64,
    },
    ExecutionsExceedAttempts {
        attempts: u64,
        executions: u64,
    },
    NonExecutedAttemptsExceedErrorOutcomes {
        non_executed: u64,
        errors: u64,
    },
    ExecuteDurationTotalOverflow,
    ExecuteDurationTotalMismatch {
        executions: u64,
        durations: u64,
    },
    PluginIdentityDoesNotMatchScope {
        scope: PluginScope,
        plugin_present: bool,
        subject_present: bool,
    },
    SessionCounts(SessionCountError),
}

impl From<SessionCountError> for PluginHookMetricsError {
    fn from(error: SessionCountError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for PluginHookMetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAttempts => formatter.write_str("plugin-hook metrics row has no attempts"),
            Self::OutcomeTotalOverflow => {
                formatter.write_str("plugin-hook outcome total overflows u64")
            }
            Self::OutcomeTotalMismatch { attempts, outcomes } => write!(
                formatter,
                "plugin-hook outcome total {outcomes} does not match {attempts} attempts"
            ),
            Self::PrepareDurationTotalOverflow => {
                formatter.write_str("plugin-hook preparation duration total overflows u64")
            }
            Self::PrepareDurationTotalMismatch {
                attempts,
                durations,
            } => write!(
                formatter,
                "plugin-hook preparation duration total {durations} does not match {attempts} attempts"
            ),
            Self::ExecutionsExceedAttempts {
                attempts,
                executions,
            } => write!(
                formatter,
                "plugin-hook executions {executions} exceed {attempts} attempts"
            ),
            Self::NonExecutedAttemptsExceedErrorOutcomes {
                non_executed,
                errors,
            } => write!(
                formatter,
                "plugin-hook non-executed attempts {non_executed} exceed {errors} error outcomes"
            ),
            Self::ExecuteDurationTotalOverflow => {
                formatter.write_str("plugin-hook execution duration total overflows u64")
            }
            Self::ExecuteDurationTotalMismatch {
                executions,
                durations,
            } => write!(
                formatter,
                "plugin-hook execution duration total {durations} does not match {executions} executions"
            ),
            Self::PluginIdentityDoesNotMatchScope {
                scope,
                plugin_present,
                subject_present,
            } => write!(
                formatter,
                "plugin coordinate and subject presence do not match plugin scope {} \
                 (coordinate present: {plugin_present}, subject present: {subject_present})",
                scope.as_str()
            ),
            Self::SessionCounts(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PluginHookMetricsError {}

#[cfg(test)]
mod tests {
    use super::super::{
        RowClassification, TelemetryRow, classify_row,
        metrics::{MAX_IDENTIFIED_SESSIONS, SessionSet, SessionSetError},
        recorded_data_example_row,
    };
    use super::*;
    fn plugin_hook_metrics_value() -> serde_json::Value {
        serde_json::from_str(recorded_data_example_row("plugin_hook_metrics")).unwrap()
    }

    fn plugin_hook_metrics_error(value: serde_json::Value) -> String {
        serde_json::from_value::<PluginHookMetricsV1>(value)
            .unwrap_err()
            .to_string()
    }

    fn validate_plugin_hook_metrics_value(
        value: serde_json::Value,
    ) -> Result<(), PluginHookMetricsError> {
        let raw = serde_json::from_value::<RawPluginHookMetricsV1>(value).unwrap();

        validate_plugin_hook_metrics(&raw)
    }

    #[test]
    fn plugin_hook_metrics_example_round_trips_through_the_classifier() {
        let source = recorded_data_example_row("plugin_hook_metrics");

        let RowClassification::Supported(TelemetryRow::PluginHookMetrics(row)) =
            classify_row(source)
        else {
            panic!("documented plugin-hook metrics row was not classified as supported");
        };
        let serialized = serde_json::to_string(&row).unwrap();

        assert_eq!(serialized, source);
    }

    #[test]
    fn plugin_hook_metrics_rejects_future_versions_unknown_fields_and_missing_fields() {
        let mut future_version = plugin_hook_metrics_value();
        future_version["v"] = serde_json::Value::from(2);
        let future_json = serde_json::to_string(&future_version).unwrap();
        let mut unknown_field = plugin_hook_metrics_value();
        unknown_field["future"] = serde_json::Value::Bool(true);
        let mut missing_field = plugin_hook_metrics_value();
        missing_field.as_object_mut().unwrap().remove("executions");

        assert_eq!(classify_row(&future_json), RowClassification::UnknownSchema);
        for value in [future_version, unknown_field, missing_field] {
            assert!(serde_json::from_value::<PluginHookMetricsV1>(value).is_err());
        }
    }

    #[test]
    fn plugin_hook_metrics_rejects_zero_attempts() {
        let mut value = plugin_hook_metrics_value();
        value["attempts"] = serde_json::Value::from(0);
        let json = serde_json::to_string(&value).unwrap();

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook metrics row has no attempts"));
        assert_eq!(classify_row(&json), RowClassification::Invalid);
    }

    #[test]
    fn plugin_hook_metrics_rejects_an_outcome_total_that_differs_from_attempts() {
        let mut value = plugin_hook_metrics_value();
        value["outcomes"]["ok"] = serde_json::Value::from(498);

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook outcome total 499 does not match 500 attempts"));
    }

    #[test]
    fn plugin_hook_metrics_rejects_an_overflowing_outcome_total() {
        let mut value = plugin_hook_metrics_value();
        value["outcomes"]["ok"] = serde_json::Value::from(u64::MAX);

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook outcome total overflows u64"));
    }

    #[test]
    fn plugin_hook_metrics_rejects_a_prepare_total_that_differs_from_attempts() {
        let mut value = plugin_hook_metrics_value();
        value["prepare_ms"]["counts"][0] = serde_json::Value::from(399);

        let error = plugin_hook_metrics_error(value);

        assert!(
            error
                .contains("plugin-hook preparation duration total 499 does not match 500 attempts")
        );
    }

    #[test]
    fn plugin_hook_metrics_rejects_an_overflowing_prepare_total() {
        let mut value = plugin_hook_metrics_value();
        value["prepare_ms"]["counts"][0] = serde_json::Value::from(u64::MAX);

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook preparation duration total overflows u64"));
    }

    #[test]
    fn plugin_hook_metrics_rejects_more_executions_than_attempts() {
        let mut value = plugin_hook_metrics_value();
        value["executions"] = serde_json::Value::from(501);

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook executions 501 exceed 500 attempts"));
    }

    #[test]
    fn plugin_hook_metrics_rejects_non_executed_attempts_without_error_outcomes() {
        let mut value = plugin_hook_metrics_value();
        value["attempts"] = serde_json::Value::from(2);
        value["executions"] = serde_json::Value::from(0);
        value["outcomes"] = serde_json::json!({"ok": 2, "blocked": 0, "error": 0});
        value["prepare_ms"]["counts"] = serde_json::json!([2, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["execute_ms"]["counts"] = serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["identified_sessions_non_ok"] = serde_json::Value::from(0);
        let json = serde_json::to_string(&value).unwrap();

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook non-executed attempts 2 exceed 0 error outcomes"));
        assert_eq!(classify_row(&json), RowClassification::Invalid);
    }

    #[test]
    fn plugin_hook_metrics_accepts_one_error_for_one_non_executed_attempt() {
        let mut value = plugin_hook_metrics_value();
        value["attempts"] = serde_json::Value::from(2);
        value["executions"] = serde_json::Value::from(1);
        value["outcomes"] = serde_json::json!({"ok": 1, "blocked": 0, "error": 1});
        value["prepare_ms"]["counts"] = serde_json::json!([2, 0, 0, 0, 0, 0, 0, 0, 0]);
        value["execute_ms"]["counts"] = serde_json::json!([1, 0, 0, 0, 0, 0, 0, 0, 0]);

        let row = serde_json::from_value::<PluginHookMetricsV1>(value);

        assert!(row.is_ok());
    }

    #[test]
    fn plugin_hook_metrics_rejects_an_execute_total_that_differs_from_executions() {
        let mut value = plugin_hook_metrics_value();
        value["execute_ms"]["counts"][7] = serde_json::Value::from(1);

        let error = plugin_hook_metrics_error(value);

        assert!(
            error
                .contains("plugin-hook execution duration total 501 does not match 500 executions")
        );
    }

    #[test]
    fn plugin_hook_metrics_rejects_an_overflowing_execute_total() {
        let mut value = plugin_hook_metrics_value();
        value["execute_ms"]["counts"][0] = serde_json::Value::from(u64::MAX);

        let error = plugin_hook_metrics_error(value);

        assert!(error.contains("plugin-hook execution duration total overflows u64"));
    }

    #[test]
    fn public_plugin_hook_metrics_require_a_coordinate_and_subject() {
        for fields_to_remove in [
            &["plugin"][..],
            &["plugin_subject"][..],
            &["plugin", "plugin_subject"][..],
        ] {
            let mut value = plugin_hook_metrics_value();
            for field in fields_to_remove {
                value.as_object_mut().unwrap().remove(*field);
            }

            let error = plugin_hook_metrics_error(value);

            assert!(error.contains("do not match plugin scope public"));
        }
    }

    #[test]
    fn unnamed_and_overflow_plugin_hook_metrics_omit_public_identity() {
        for scope in [PluginScope::Unnamed, PluginScope::Overflow] {
            let mut value = plugin_hook_metrics_value();
            value["plugin_scope"] = serde_json::to_value(scope).unwrap();
            value.as_object_mut().unwrap().remove("plugin");
            value.as_object_mut().unwrap().remove("plugin_subject");

            let row = serde_json::from_value::<PluginHookMetricsV1>(value).unwrap();
            let serialized = serde_json::to_value(row).unwrap();

            assert!(serialized.get("plugin").is_none());
            assert!(serialized.get("plugin_subject").is_none());
        }
    }

    #[test]
    fn unnamed_and_overflow_plugin_hook_metrics_reject_public_identity() {
        for scope in [PluginScope::Unnamed, PluginScope::Overflow] {
            for field_to_keep in ["plugin", "plugin_subject"] {
                let mut value = plugin_hook_metrics_value();
                value["plugin_scope"] = serde_json::to_value(scope).unwrap();
                for field in ["plugin", "plugin_subject"] {
                    if field != field_to_keep {
                        value.as_object_mut().unwrap().remove(field);
                    }
                }

                let error = plugin_hook_metrics_error(value);

                assert!(error.contains(&format!("do not match plugin scope {}", scope.as_str())));
            }
        }
    }

    #[test]
    fn plugin_hook_metrics_apply_the_shared_session_count_rules() {
        let mut value = plugin_hook_metrics_value();
        value["identified_sessions"] = serde_json::Value::from(501);

        let result = validate_plugin_hook_metrics_value(value);

        assert_eq!(
            result,
            Err(PluginHookMetricsError::SessionCounts(
                SessionCountError::SessionSet(SessionSetError::ExceedsLimit {
                    set: SessionSet::Identified,
                    identified: 501,
                    maximum: MAX_IDENTIFIED_SESSIONS,
                })
            ))
        );
    }
}
