//! Atomic write-side updates for plugin-hook aggregate rows.

use std::fmt;

use super::{
    PluginHookMetricsV1, PluginHookOutcome,
    outcome::{PluginHookAttempt, PluginHookOutcomeCounterOverflow, PluginHookOutcomeCounters},
};
use crate::telemetry::{
    schema::{
        SchemaVersion, SymposiumVersion, UtcDay,
        agent::{VendorSessionId, derive_session_id},
        metrics::{LatencyHistogram, LatencyHistogramError},
    },
    state::{
        BoundRecordingObservation, HookSessionCountSnapshot, HookSessionCountUpdateError,
        SelectedPluginHookAggregate,
    },
};

/// One completed plugin-hook attempt added to an aggregate row.
///
/// The admitted selection supplies the agent, hook surface, and plugin
/// identity. This value carries only the process result and optional vendor
/// session identifier observed for the attempt.
// Intentionally omit `Debug`: this value borrows a raw vendor session id.
#[derive(Clone)]
pub(in crate::telemetry) struct PluginHookMetricObservation<'a> {
    pub(in crate::telemetry) attempt: PluginHookAttempt,
    pub(in crate::telemetry) vendor_session_id: Option<&'a VendorSessionId>,
}

impl PluginHookMetricsV1 {
    /// Start an aggregate row with its first completed plugin-hook attempt.
    ///
    /// The selected private-state entry supplies the stable row identifier,
    /// agent, hook surface, admitted plugin identity, and session tracker
    /// together. The recording verifies the selected identifier epoch and
    /// derives an optional scoped session identifier.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHookMetricsUpdateError`] when any counter cannot
    /// represent the attempt or the supplied context selects another row.
    #[must_use = "a failed plugin-hook metric update must be dropped"]
    pub(in crate::telemetry) fn new(
        recording: &BoundRecordingObservation<'_>,
        observation: PluginHookMetricObservation<'_>,
        selected: SelectedPluginHookAggregate<'_>,
    ) -> Result<Self, PluginHookMetricsUpdateError> {
        let bucket = selected.bucket();
        let mut row = Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: selected.event_id(),
            day: selected.day(),
            symposium: SymposiumVersion::current(),
            agent: selected.agent(),
            hook: selected.surface(),
            plugin_scope: bucket.scope(),
            plugin: bucket.plugin().cloned(),
            attempts: 0,
            executions: 0,
            outcomes: PluginHookOutcomeCounters::default(),
            prepare_ms: LatencyHistogram::default(),
            execute_ms: LatencyHistogram::default(),
            identified_sessions: None,
            identified_sessions_non_ok: None,
            session_counts_complete: false,
            plugin_subject: bucket.plugin_subject(),
        };

        row.checked_record(recording, observation, selected)?;
        Ok(row)
    }

    /// Add one plugin-hook attempt without partially changing its row or
    /// private session tracker.
    ///
    /// Every row counter is updated on a clone first. The tracker update is the
    /// final fallible operation and remains unchanged on failure; publishing
    /// its snapshot and replacing the row are then infallible. Passing the
    /// complete selected entry keeps its event identifier and tracker paired.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHookMetricsUpdateError`] when the recording or private
    /// state selects another aggregate, or when a counter would overflow. Both
    /// the row and selected private state remain unchanged on failure.
    #[must_use = "a failed plugin-hook metric update must be dropped"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        observation: PluginHookMetricObservation<'_>,
        mut selected: SelectedPluginHookAggregate<'_>,
    ) -> Result<(), PluginHookMetricsUpdateError> {
        self.ensure_selected_by(recording, &selected)?;

        let outcome = observation.attempt.outcome();
        let mut next = self.clone();
        next.attempts = next
            .attempts
            .checked_add(1)
            .ok_or(PluginHookMetricsUpdateError::AttemptCountOverflow)?;
        next.outcomes.checked_record(outcome)?;
        next.prepare_ms
            .checked_record(observation.attempt.prepare_duration())
            .map_err(PluginHookMetricsUpdateError::PrepareDuration)?;

        if let Some(duration) = observation.attempt.execute_duration() {
            next.executions = next
                .executions
                .checked_add(1)
                .ok_or(PluginHookMetricsUpdateError::ExecutionCountOverflow)?;
            next.execute_ms
                .checked_record(duration)
                .map_err(PluginHookMetricsUpdateError::ExecuteDuration)?;
        }

        let session_id = derive_session_id(
            recording.identifier_window_scope(),
            selected.agent(),
            observation.vendor_session_id,
        );
        // `attempts + 1` succeeded above, so the tracker's matching
        // contribution increment cannot overflow here.
        let session_counts = selected.checked_record_session(self.attempts, session_id, outcome)?;
        next.apply_session_counts(session_counts);

        *self = next;
        Ok(())
    }

    fn ensure_selected_by(
        &self,
        recording: &BoundRecordingObservation<'_>,
        selected: &SelectedPluginHookAggregate<'_>,
    ) -> Result<(), PluginHookMetricsUpdateError> {
        if !selected.matches_recording(recording) {
            return Err(PluginHookMetricsUpdateError::RecordingContextChanged);
        }
        if self.day != selected.day() {
            return Err(PluginHookMetricsUpdateError::DayChanged {
                row_day: self.day,
                observation_day: selected.day(),
            });
        }
        if self.agent != selected.agent() || self.hook != selected.surface() {
            return Err(PluginHookMetricsUpdateError::TargetChanged);
        }

        let bucket = selected.bucket();
        if self.plugin_scope != bucket.scope()
            || self.plugin.as_ref() != bucket.plugin()
            || self.plugin_subject != bucket.plugin_subject()
        {
            return Err(PluginHookMetricsUpdateError::PluginIdentityChanged);
        }
        if self.event_id != selected.event_id() {
            return Err(PluginHookMetricsUpdateError::RowIdentifierChanged);
        }

        Ok(())
    }

    fn apply_session_counts(&mut self, snapshot: HookSessionCountSnapshot) {
        match snapshot {
            HookSessionCountSnapshot::Complete {
                identified_sessions,
                identified_sessions_non_ok,
            } => {
                self.identified_sessions = Some(identified_sessions);
                self.identified_sessions_non_ok = Some(identified_sessions_non_ok);
                self.session_counts_complete = true;
            }
            HookSessionCountSnapshot::Incomplete => {
                self.identified_sessions = None;
                self.identified_sessions_non_ok = None;
                self.session_counts_complete = false;
            }
        }
    }
}

/// A plugin-hook aggregate update that cannot be represented safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookMetricsUpdateError {
    RecordingContextChanged,
    DayChanged {
        row_day: UtcDay,
        observation_day: UtcDay,
    },
    TargetChanged,
    PluginIdentityChanged,
    RowIdentifierChanged,
    AttemptCountOverflow,
    ExecutionCountOverflow,
    OutcomeCountOverflow {
        outcome: PluginHookOutcome,
    },
    PrepareDuration(LatencyHistogramError),
    ExecuteDuration(LatencyHistogramError),
    SessionCounts(HookSessionCountUpdateError),
}

impl From<PluginHookOutcomeCounterOverflow> for PluginHookMetricsUpdateError {
    fn from(error: PluginHookOutcomeCounterOverflow) -> Self {
        Self::OutcomeCountOverflow {
            outcome: error.outcome,
        }
    }
}

impl From<HookSessionCountUpdateError> for PluginHookMetricsUpdateError {
    fn from(error: HookSessionCountUpdateError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for PluginHookMetricsUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordingContextChanged => {
                formatter.write_str("plugin-hook recording context changed before update")
            }
            Self::DayChanged {
                row_day,
                observation_day,
            } => write!(
                formatter,
                "plugin-hook metrics row belongs to {row_day}, not observed day {observation_day}"
            ),
            Self::TargetChanged => {
                formatter.write_str("plugin-hook aggregate target changed during update")
            }
            Self::PluginIdentityChanged => {
                formatter.write_str("plugin-hook aggregate identity changed during update")
            }
            Self::RowIdentifierChanged => {
                formatter.write_str("plugin-hook private state belongs to another aggregate row")
            }
            Self::AttemptCountOverflow => {
                formatter.write_str("plugin-hook attempt count overflows u64")
            }
            Self::ExecutionCountOverflow => {
                formatter.write_str("plugin-hook execution count overflows u64")
            }
            Self::OutcomeCountOverflow { outcome } => write!(
                formatter,
                "{} plugin-hook outcome counter overflows u64",
                outcome.counter_name()
            ),
            Self::PrepareDuration(error) => error.fmt(formatter),
            Self::ExecuteDuration(error) => error.fmt(formatter),
            Self::SessionCounts(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PluginHookMetricsUpdateError {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::{
        HookAgent, HookSurface, PluginHookAttribution, PluginScope, PublicPluginCoordinate,
        outcome::PluginHookOutcomeSignals,
    };
    use super::*;
    use crate::telemetry::{
        schema::{
            IDENTIFIER_WINDOW_TEST_STATE, RowClassification, TelemetryRow, UtcSecond, classify_row,
            extension::PublicExtensionSource, recording_observation,
        },
        state::{PluginHookAggregateStore, TelemetryStateV1},
    };
    use chrono::{TimeZone as _, Utc};

    fn public_plugin(source: PublicExtensionSource, name: &str) -> PublicPluginCoordinate {
        PublicPluginCoordinate::try_new(source, name).unwrap()
    }

    fn successful_attempt() -> PluginHookAttempt {
        PluginHookAttempt::Executed {
            signals: PluginHookOutcomeSignals {
                error: false,
                blocked: false,
            },
            prepare_duration: Duration::from_millis(1),
            execute_duration: Duration::from_millis(1),
        }
    }

    fn metric_observation<'a>(
        attempt: PluginHookAttempt,
        vendor_session_id: Option<&'a VendorSessionId>,
    ) -> PluginHookMetricObservation<'a> {
        PluginHookMetricObservation {
            attempt,
            vendor_session_id,
        }
    }

    fn aggregate_store(recording: &BoundRecordingObservation<'_>) -> PluginHookAggregateStore {
        PluginHookAggregateStore::new(recording.day())
    }

    fn select_private<'a>(
        store: &'a mut PluginHookAggregateStore,
        recording: &BoundRecordingObservation<'_>,
        attribution: PluginHookAttribution,
        hook: HookSurface,
    ) -> SelectedPluginHookAggregate<'a> {
        store
            .select(recording, HookAgent::Claude, hook, attribution)
            .unwrap()
    }

    fn initialized_aggregate(
        recording: &BoundRecordingObservation<'_>,
        attribution: PluginHookAttribution,
    ) -> (PluginHookMetricsV1, PluginHookAggregateStore) {
        let observation = metric_observation(successful_attempt(), None);
        let mut store = aggregate_store(recording);
        let selected = select_private(&mut store, recording, attribution, HookSurface::PreToolUse);
        let row = PluginHookMetricsV1::new(recording, observation, selected).unwrap();

        (row, store)
    }

    fn assert_update_rejected_without_mutation(
        row: &mut PluginHookMetricsV1,
        recording: &BoundRecordingObservation<'_>,
        observation: PluginHookMetricObservation<'_>,
        store: &mut PluginHookAggregateStore,
        attribution: PluginHookAttribution,
        hook: HookSurface,
        expected: PluginHookMetricsUpdateError,
    ) {
        let row_before = row.clone();
        drop(select_private(store, recording, attribution.clone(), hook));
        let store_before = store.clone();
        let selected = select_private(store, recording, attribution, hook);

        let result = row.checked_record(recording, observation, selected);

        assert_eq!(result, Err(expected));
        assert_eq!(*row, row_before);
        assert_eq!(*store, store_before);
    }

    #[test]
    fn first_unexecuted_plugin_attempt_populates_an_error_aggregate() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let plugin = public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        );
        let attribution = PluginHookAttribution::Public(plugin.clone());
        let observation = metric_observation(
            PluginHookAttempt::NotExecuted {
                prepare_duration: Duration::from_millis(7),
            },
            Some(&vendor_session_id),
        );
        let mut store = aggregate_store(&recording);
        let event_id = select_private(
            &mut store,
            &recording,
            attribution.clone(),
            HookSurface::PreToolUse,
        )
        .event_id();
        let selected = select_private(&mut store, &recording, attribution, HookSurface::PreToolUse);

        let row = PluginHookMetricsV1::new(&recording, observation, selected).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();

        assert_eq!(row.event_id, event_id);
        assert_eq!(row.day, recording.day());
        assert_eq!(row.plugin_scope, PluginScope::Public);
        assert_eq!(row.plugin, Some(plugin));
        assert_eq!(row.attempts, 1);
        assert_eq!(row.executions, 0);
        assert_eq!(
            value["outcomes"],
            serde_json::json!({
                "ok": 0,
                "blocked": 0,
                "error": 1,
            })
        );
        assert_eq!(
            value["prepare_ms"]["counts"],
            serde_json::json!([0, 1, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            value["execute_ms"]["counts"],
            serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(row.identified_sessions, Some(1));
        assert_eq!(row.identified_sessions_non_ok, Some(1));
        assert!(row.session_counts_complete);
        assert!(matches!(
            classify_row(&json),
            RowClassification::Supported(TelemetryRow::PluginHookMetrics(_))
        ));
    }

    #[test]
    fn executed_plugin_attempts_accumulate_every_metric() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let first_session = VendorSessionId::new("vendor-session-123".to_owned());
        let second_session = VendorSessionId::new("vendor-session-456".to_owned());
        let attribution = PluginHookAttribution::Public(public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        ));
        let first = metric_observation(
            PluginHookAttempt::Executed {
                signals: PluginHookOutcomeSignals {
                    error: false,
                    blocked: false,
                },
                prepare_duration: Duration::from_millis(5),
                execute_duration: Duration::from_millis(25),
            },
            Some(&first_session),
        );
        let mut store = aggregate_store(&recording);
        let selected = select_private(
            &mut store,
            &recording,
            attribution.clone(),
            HookSurface::PreToolUse,
        );
        let mut row = PluginHookMetricsV1::new(&recording, first, selected).unwrap();
        let event_id = row.event_id;
        let second = metric_observation(
            PluginHookAttempt::Executed {
                signals: PluginHookOutcomeSignals {
                    error: false,
                    blocked: true,
                },
                prepare_duration: Duration::from_millis(1_001),
                execute_duration: Duration::from_millis(6),
            },
            Some(&second_session),
        );

        let selected = select_private(&mut store, &recording, attribution, HookSurface::PreToolUse);
        row.checked_record(&recording, second, selected).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();

        assert_eq!(row.event_id, event_id);
        assert_eq!(row.attempts, 2);
        assert_eq!(row.executions, 2);
        assert_eq!(
            value["outcomes"],
            serde_json::json!({
                "ok": 1,
                "blocked": 1,
                "error": 0,
            })
        );
        assert_eq!(
            value["prepare_ms"]["counts"],
            serde_json::json!([1, 0, 0, 0, 0, 0, 0, 0, 1])
        );
        assert_eq!(
            value["execute_ms"]["counts"],
            serde_json::json!([0, 1, 1, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(row.identified_sessions, Some(2));
        assert_eq!(row.identified_sessions_non_ok, Some(1));
        assert!(row.session_counts_complete);
        assert!(matches!(
            classify_row(&json),
            RowClassification::Supported(TelemetryRow::PluginHookMetrics(_))
        ));
    }

    #[test]
    fn missing_session_id_makes_plugin_hook_counts_incomplete() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let observation = metric_observation(
            PluginHookAttempt::Executed {
                signals: PluginHookOutcomeSignals {
                    error: false,
                    blocked: false,
                },
                prepare_duration: Duration::from_millis(1),
                execute_duration: Duration::from_millis(1),
            },
            None,
        );
        let attribution = PluginHookAttribution::Unnamed;
        let mut store = aggregate_store(&recording);
        let selected = select_private(&mut store, &recording, attribution, HookSurface::PreToolUse);

        let row = PluginHookMetricsV1::new(&recording, observation, selected).unwrap();

        assert!(!row.session_counts_complete);
        assert_eq!(row.identified_sessions, None);
        assert_eq!(row.identified_sessions_non_ok, None);
    }

    #[test]
    fn another_plugin_identity_is_rejected_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let first_plugin = PluginHookAttribution::Public(public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        ));
        let second_plugin = PluginHookAttribution::Public(public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "another-tools",
        ));
        let (mut row, _) = initialized_aggregate(&recording, first_plugin);
        let observation = metric_observation(successful_attempt(), None);
        let mut store = aggregate_store(&recording);

        assert_update_rejected_without_mutation(
            &mut row,
            &recording,
            observation,
            &mut store,
            second_plugin,
            HookSurface::PreToolUse,
            PluginHookMetricsUpdateError::PluginIdentityChanged,
        );
    }

    #[test]
    fn another_day_is_rejected_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let (mut row, _) = {
            let recording = recording_observation(&mut state);
            initialized_aggregate(&recording, PluginHookAttribution::Unnamed)
        };
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 4, 10, 2, 11).unwrap());
        let observation = state.observe_recording(completed_at).unwrap();
        let recording = state.bind_recording_observation(observation).unwrap();
        let observation = metric_observation(successful_attempt(), None);
        let mut store = aggregate_store(&recording);
        let row_day = row.day;

        assert_update_rejected_without_mutation(
            &mut row,
            &recording,
            observation,
            &mut store,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
            PluginHookMetricsUpdateError::DayChanged {
                row_day,
                observation_day: recording.day(),
            },
        );
    }

    #[test]
    fn another_hook_target_is_rejected_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let (mut row, _) = initialized_aggregate(&recording, PluginHookAttribution::Unnamed);
        let observation = metric_observation(successful_attempt(), None);
        let mut store = aggregate_store(&recording);

        assert_update_rejected_without_mutation(
            &mut row,
            &recording,
            observation,
            &mut store,
            PluginHookAttribution::Unnamed,
            HookSurface::PostToolUse,
            PluginHookMetricsUpdateError::TargetChanged,
        );
    }

    #[test]
    fn reset_epoch_uses_event_id_to_reject_an_old_unnamed_row() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let (mut row, mut store) = {
            let recording = recording_observation(&mut state);
            let observation = metric_observation(
                PluginHookAttempt::Executed {
                    signals: PluginHookOutcomeSignals {
                        error: false,
                        blocked: false,
                    },
                    prepare_duration: Duration::from_millis(1),
                    execute_duration: Duration::from_millis(1),
                },
                None,
            );
            let mut store = aggregate_store(&recording);
            let selected = select_private(
                &mut store,
                &recording,
                PluginHookAttribution::Unnamed,
                HookSurface::PreToolUse,
            );

            (
                PluginHookMetricsV1::new(&recording, observation, selected).unwrap(),
                store,
            )
        };
        state.reset_identifiers(row.day).unwrap();
        store.reset_identifier_epoch();
        let recording = recording_observation(&mut state);
        let observation = metric_observation(
            PluginHookAttempt::Executed {
                signals: PluginHookOutcomeSignals {
                    error: false,
                    blocked: false,
                },
                prepare_duration: Duration::from_millis(1),
                execute_duration: Duration::from_millis(1),
            },
            None,
        );
        let row_before = row.clone();
        drop(select_private(
            &mut store,
            &recording,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
        ));
        let store_before = store.clone();
        let selected = select_private(
            &mut store,
            &recording,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
        );

        let result = row.checked_record(&recording, observation, selected);

        assert_eq!(
            result,
            Err(PluginHookMetricsUpdateError::RowIdentifierChanged)
        );
        assert_eq!(row, row_before);
        assert_eq!(store, store_before);
    }

    #[test]
    fn update_rejects_a_selection_from_another_identifier_epoch() {
        let mut old_state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let old_recording = recording_observation(&mut old_state);
        let (mut row, mut store) =
            initialized_aggregate(&old_recording, PluginHookAttribution::Unnamed);

        let mut reset_state: TelemetryStateV1 =
            toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        reset_state.reset_identifiers(row.day).unwrap();
        let reset_recording = recording_observation(&mut reset_state);
        let observation = metric_observation(successful_attempt(), None);
        let row_before = row.clone();
        let store_before = store.clone();
        let selected = select_private(
            &mut store,
            &old_recording,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
        );

        let result = row.checked_record(&reset_recording, observation, selected);

        assert_eq!(
            result,
            Err(PluginHookMetricsUpdateError::RecordingContextChanged)
        );
        assert_eq!(row, row_before);
        assert_eq!(store, store_before);
    }

    #[test]
    fn failed_prepare_histogram_update_preserves_row_and_private_state() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let first = metric_observation(
            PluginHookAttempt::Executed {
                signals: PluginHookOutcomeSignals {
                    error: false,
                    blocked: false,
                },
                prepare_duration: Duration::from_millis(1),
                execute_duration: Duration::from_millis(1),
            },
            Some(&vendor_session_id),
        );
        let mut store = aggregate_store(&recording);
        let selected = select_private(
            &mut store,
            &recording,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
        );
        let mut row = PluginHookMetricsV1::new(&recording, first, selected).unwrap();
        row.prepare_ms = serde_json::from_value(serde_json::json!({
            "bounds": [5, 10, 25, 50, 100, 250, 500, 1000],
            "counts": [u64::MAX, 0, 0, 0, 0, 0, 0, 0, 0],
        }))
        .unwrap();
        let row_before = row.clone();
        let store_before = store.clone();
        let second = metric_observation(
            PluginHookAttempt::NotExecuted {
                prepare_duration: Duration::from_millis(1),
            },
            Some(&vendor_session_id),
        );
        let selected = select_private(
            &mut store,
            &recording,
            PluginHookAttribution::Unnamed,
            HookSurface::PreToolUse,
        );

        let result = row.checked_record(&recording, second, selected);

        assert_eq!(
            result,
            Err(PluginHookMetricsUpdateError::PrepareDuration(
                LatencyHistogramError::BucketCountOverflow { bucket: 0 }
            ))
        );
        assert_eq!(row, row_before);
        assert_eq!(store, store_before);
    }
}
