//! Atomic write-side updates for hook aggregate rows.

use std::{fmt, time::Duration};

use super::{
    HookMetricsV1,
    key::HookMetricsKey,
    outcome::{HookOutcome, HookOutcomeCounterOverflow, HookOutcomeCounters},
    surface::HookSurface,
};
use crate::telemetry::{
    schema::{
        EventId, SchemaVersion, SymposiumVersion, UtcDay,
        agent::{HookAgent, VendorSessionId, derive_session_id},
        metrics::{LatencyHistogram, LatencyHistogramError},
    },
    state::{
        BoundRecordingObservation, HookSessionCountSnapshot, HookSessionCountTracker,
        HookSessionCountUpdateError,
    },
};

/// One completed top-level hook observation added to an aggregate row.
///
/// Named fields keep the agent, surface, counters, duration, and optional
/// vendor session identifier together at the write boundary.
// Intentionally omit `Debug`: this value borrows a raw vendor session id.
#[derive(Clone, Copy)]
pub(in crate::telemetry) struct HookMetricObservation<'a> {
    pub(in crate::telemetry) agent: HookAgent,
    pub(in crate::telemetry) hook: HookSurface,
    pub(in crate::telemetry) outcome: HookOutcome,
    pub(in crate::telemetry) plugins_attempted: u64,
    pub(in crate::telemetry) plugins_completed: u64,
    pub(in crate::telemetry) duration: Duration,
    pub(in crate::telemetry) vendor_session_id: Option<&'a VendorSessionId>,
}

impl HookMetricsV1 {
    /// Start an aggregate row with its first completed hook observation.
    ///
    /// The row day, hook subject, and optional session identifier all come
    /// from the same bound recording context. A private tracker that survives
    /// without its snapshot should be supplied here; the contribution-count
    /// mismatch makes its published session counts incomplete.
    ///
    /// # Errors
    ///
    /// Returns [`HookMetricsUpdateError`] when any counter cannot represent
    /// the observation or the supplied context does not select this row.
    #[must_use = "a failed hook metric update must be dropped"]
    pub(in crate::telemetry) fn new(
        recording: &BoundRecordingObservation<'_>,
        observation: HookMetricObservation<'_>,
        session_counts: &mut HookSessionCountTracker<HookMetricsKey>,
    ) -> Result<Self, HookMetricsUpdateError> {
        let key = HookMetricsKey::new(recording, observation.agent, observation.hook);
        let mut row = Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day: recording.day(),
            symposium: SymposiumVersion::current(),
            agent: observation.agent,
            hook: observation.hook,
            invocations: 0,
            outcomes: HookOutcomeCounters::default(),
            plugins_attempted: 0,
            plugins_completed: 0,
            duration_ms: LatencyHistogram::default(),
            identified_sessions: None,
            identified_sessions_non_ok: None,
            session_counts_complete: false,
            hook_subject: key.hook_subject,
        };

        row.checked_record(recording, observation, session_counts)?;
        Ok(row)
    }

    /// Add one completed hook observation without partially changing either
    /// the row or its private session tracker.
    ///
    /// All row counters are updated on a clone first. The tracker update is
    /// the final fallible operation and guarantees it remains unchanged on
    /// failure; applying its resulting snapshot and replacing the row are
    /// infallible. The current row invocation count is always supplied to the
    /// tracker, making state/snapshot reconciliation part of every update.
    ///
    /// # Errors
    ///
    /// Returns [`HookMetricsUpdateError`] when the recording selects another
    /// aggregate or any counter would overflow. Both inputs remain unchanged
    /// on failure.
    #[must_use = "a failed hook metric update must be dropped"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        observation: HookMetricObservation<'_>,
        session_counts: &mut HookSessionCountTracker<HookMetricsKey>,
    ) -> Result<(), HookMetricsUpdateError> {
        self.ensure_selected_by(recording, observation, session_counts)?;

        if observation.plugins_completed > observation.plugins_attempted {
            return Err(HookMetricsUpdateError::CompletedPluginsExceedAttempts {
                attempted: observation.plugins_attempted,
                completed: observation.plugins_completed,
            });
        }

        let mut next = self.clone();
        next.invocations = next
            .invocations
            .checked_add(1)
            .ok_or(HookMetricsUpdateError::InvocationCountOverflow)?;
        next.outcomes.checked_record(observation.outcome)?;
        next.plugins_attempted = next
            .plugins_attempted
            .checked_add(observation.plugins_attempted)
            .ok_or(HookMetricsUpdateError::PluginAttemptCountOverflow)?;
        next.plugins_completed = next
            .plugins_completed
            .checked_add(observation.plugins_completed)
            .ok_or(HookMetricsUpdateError::PluginCompletionCountOverflow)?;
        next.duration_ms.checked_record(observation.duration)?;

        let session_id = derive_session_id(
            recording.identifier_window_scope(),
            observation.agent,
            observation.vendor_session_id,
        );
        // `invocations + 1` succeeded above, so the tracker's matching
        // contribution increment cannot overflow here.
        session_counts.checked_record(self.invocations, session_id, observation.outcome)?;
        next.apply_session_counts(session_counts.snapshot());

        *self = next;
        Ok(())
    }

    fn ensure_selected_by(
        &self,
        recording: &BoundRecordingObservation<'_>,
        observation: HookMetricObservation<'_>,
        session_counts: &HookSessionCountTracker<HookMetricsKey>,
    ) -> Result<(), HookMetricsUpdateError> {
        if self.day != recording.day() {
            return Err(HookMetricsUpdateError::DayChanged {
                row_day: self.day,
                observation_day: recording.day(),
            });
        }
        if self.agent != observation.agent || self.hook != observation.hook {
            return Err(HookMetricsUpdateError::TargetChanged);
        }

        let selected_key = HookMetricsKey::new(recording, observation.agent, observation.hook);
        if self.key() != selected_key {
            return Err(HookMetricsUpdateError::IdentifierEpochChanged);
        }
        if session_counts.key() != &selected_key {
            return Err(HookMetricsUpdateError::SessionTrackerChanged);
        }

        Ok(())
    }

    /// Return the lookup key shared with this aggregate's private state.
    #[must_use]
    pub(in crate::telemetry) const fn key(&self) -> HookMetricsKey {
        HookMetricsKey {
            day: self.day,
            hook_subject: self.hook_subject,
        }
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

/// A hook aggregate update that cannot be represented safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum HookMetricsUpdateError {
    DayChanged {
        row_day: UtcDay,
        observation_day: UtcDay,
    },
    TargetChanged,
    IdentifierEpochChanged,
    SessionTrackerChanged,
    InvocationCountOverflow,
    OutcomeCountOverflow {
        outcome: HookOutcome,
    },
    PluginAttemptCountOverflow,
    PluginCompletionCountOverflow,
    CompletedPluginsExceedAttempts {
        attempted: u64,
        completed: u64,
    },
    Duration(LatencyHistogramError),
    SessionCounts(HookSessionCountUpdateError),
}

impl From<HookOutcomeCounterOverflow> for HookMetricsUpdateError {
    fn from(error: HookOutcomeCounterOverflow) -> Self {
        Self::OutcomeCountOverflow {
            outcome: error.outcome,
        }
    }
}

impl From<LatencyHistogramError> for HookMetricsUpdateError {
    fn from(error: LatencyHistogramError) -> Self {
        Self::Duration(error)
    }
}

impl From<HookSessionCountUpdateError> for HookMetricsUpdateError {
    fn from(error: HookSessionCountUpdateError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for HookMetricsUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayChanged {
                row_day,
                observation_day,
            } => write!(
                formatter,
                "hook metrics row belongs to {row_day}, not observed day {observation_day}"
            ),
            Self::TargetChanged => {
                formatter.write_str("hook observation targets another agent or hook surface")
            }
            Self::IdentifierEpochChanged => {
                formatter.write_str("hook observation belongs to another identifier epoch")
            }
            Self::SessionTrackerChanged => {
                formatter.write_str("hook session tracker belongs to another aggregate")
            }
            Self::InvocationCountOverflow => {
                formatter.write_str("hook invocation count overflows u64")
            }
            Self::OutcomeCountOverflow { outcome } => write!(
                formatter,
                "{} hook outcome counter overflows u64",
                outcome.counter_name()
            ),
            Self::PluginAttemptCountOverflow => {
                formatter.write_str("attempted plugin hook count overflows u64")
            }
            Self::PluginCompletionCountOverflow => {
                formatter.write_str("completed plugin hook count overflows u64")
            }
            Self::CompletedPluginsExceedAttempts {
                attempted,
                completed,
            } => write!(
                formatter,
                "completed plugin hooks {completed} exceed {attempted} attempted plugin hooks"
            ),
            Self::Duration(error) => fmt::Display::fmt(error, formatter),
            Self::SessionCounts(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for HookMetricsUpdateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{
        schema::{
            IDENTIFIER_WINDOW_TEST_STATE, RowClassification, RowKind, TelemetryRow, UtcSecond,
            classify_row, recording_observation,
        },
        state::{HookAggregateStore, TelemetryStateV1},
    };
    use chrono::{TimeZone as _, Utc};

    fn metric_observation<'a>(
        outcome: HookOutcome,
        vendor_session_id: Option<&'a VendorSessionId>,
    ) -> HookMetricObservation<'a> {
        HookMetricObservation {
            agent: HookAgent::Claude,
            hook: HookSurface::PreToolUse,
            outcome,
            plugins_attempted: 1,
            plugins_completed: 1,
            duration: Duration::from_millis(5),
            vendor_session_id,
        }
    }

    fn session_counts(
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        hook: HookSurface,
    ) -> HookSessionCountTracker<HookMetricsKey> {
        HookSessionCountTracker::new(HookMetricsKey::new(recording, agent, hook))
    }

    #[test]
    fn first_hook_observation_populates_the_complete_aggregate() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let input = metric_observation(HookOutcome::Blocked, Some(&vendor_session_id));

        let row = HookMetricsV1::new(&recording, input, &mut session_counts).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::HookMetrics);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, recording.day());
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.agent, HookAgent::Claude);
        assert_eq!(row.hook, HookSurface::PreToolUse);
        assert_eq!(row.invocations, 1);
        assert_eq!(
            value["outcomes"],
            serde_json::json!({
                "ok": 0,
                "blocked": 1,
                "plugin_error": 0,
                "internal_error": 0,
            })
        );
        assert_eq!(row.plugins_attempted, 1);
        assert_eq!(row.plugins_completed, 1);
        assert_eq!(
            value["duration_ms"]["counts"],
            serde_json::json!([1, 0, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(row.identified_sessions, Some(1));
        assert_eq!(row.identified_sessions_non_ok, Some(1));
        assert!(row.session_counts_complete);
        assert_eq!(
            row.hook_subject,
            "hok_99106b457209912cf0023c562a72dafb".parse().unwrap()
        );
        assert!(matches!(
            classify_row(&json),
            RowClassification::Supported(TelemetryRow::HookMetrics(_))
        ));
    }

    #[test]
    fn hook_store_supplies_trackers_for_creation_updates_and_day_rollover() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut store;
        let mut row;
        {
            let recording = recording_observation(&mut state);
            store = HookAggregateStore::new(recording.day());
            let tracker = store
                .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();
            row = HookMetricsV1::new(
                &recording,
                metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
                tracker,
            )
            .unwrap();
            let tracker = store
                .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();

            row.checked_record(
                &recording,
                metric_observation(HookOutcome::Blocked, Some(&vendor_session_id)),
                tracker,
            )
            .unwrap();
        }
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 4, 10, 2, 11).unwrap());
        let observation = state.observe_recording(completed_at).unwrap();
        let later = state.bind_recording_observation(observation).unwrap();
        let row_day = row.day;
        let tracker = store
            .select(&later, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();
        let tracker_before = tracker.clone();

        let result = row.checked_record(
            &later,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            tracker,
        );

        assert_eq!(row.invocations, 2);
        assert_eq!(
            result,
            Err(HookMetricsUpdateError::DayChanged {
                row_day,
                observation_day: later.day(),
            })
        );
        assert_eq!(&*tracker, &tracker_before);
    }

    #[test]
    fn later_hook_observations_accumulate_every_metric() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let first_session = VendorSessionId::new("vendor-session-123".to_owned());
        let second_session = VendorSessionId::new("vendor-session-456".to_owned());
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let mut row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&first_session)),
            &mut session_counts,
        )
        .unwrap();
        let event_id = row.event_id;
        let second = HookMetricObservation {
            plugins_attempted: 3,
            plugins_completed: 2,
            duration: Duration::from_millis(1_001),
            ..metric_observation(HookOutcome::Blocked, Some(&second_session))
        };

        row.checked_record(&recording, second, &mut session_counts)
            .unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();

        assert_eq!(row.event_id, event_id);
        assert_eq!(row.invocations, 2);
        assert_eq!(
            value["outcomes"],
            serde_json::json!({
                "ok": 1,
                "blocked": 1,
                "plugin_error": 0,
                "internal_error": 0,
            })
        );
        assert_eq!(row.plugins_attempted, 4);
        assert_eq!(row.plugins_completed, 3);
        assert_eq!(
            value["duration_ms"]["counts"],
            serde_json::json!([1, 0, 0, 0, 0, 0, 0, 0, 1])
        );
        assert_eq!(row.identified_sessions, Some(2));
        assert_eq!(row.identified_sessions_non_ok, Some(1));
        assert!(row.session_counts_complete);
        assert!(matches!(
            classify_row(&json),
            RowClassification::Supported(TelemetryRow::HookMetrics(_))
        ));
    }

    #[test]
    fn missing_session_id_makes_the_first_aggregate_incomplete() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);

        let row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, None),
            &mut session_counts,
        )
        .unwrap();
        let value = serde_json::to_value(row).unwrap();

        assert_eq!(value["session_counts_complete"], false);
        assert!(value.get("identified_sessions").is_none());
        assert!(value.get("identified_sessions_non_ok").is_none());
    }

    #[test]
    fn snapshot_mismatch_makes_session_counts_incomplete_during_row_update() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let mut row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut session_counts,
        )
        .unwrap();
        let session_id = derive_session_id(
            recording.identifier_window_scope(),
            HookAgent::Claude,
            Some(&vendor_session_id),
        );
        session_counts
            .checked_record(1, session_id, HookOutcome::Ok)
            .unwrap();

        row.checked_record(
            &recording,
            metric_observation(HookOutcome::Blocked, Some(&vendor_session_id)),
            &mut session_counts,
        )
        .unwrap();

        assert_eq!(row.invocations, 2);
        assert!(!row.session_counts_complete);
        assert_eq!(row.identified_sessions, None);
        assert_eq!(row.identified_sessions_non_ok, None);
    }

    #[test]
    fn failed_histogram_update_preserves_the_row_and_session_tracker() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let mut row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut session_counts,
        )
        .unwrap();
        row.duration_ms = serde_json::from_value(serde_json::json!({
            "bounds": [5, 10, 25, 50, 100, 250, 500, 1000],
            "counts": [u64::MAX, 0, 0, 0, 0, 0, 0, 0, 0],
        }))
        .unwrap();
        let row_before = row.clone();
        let session_counts_before = session_counts.clone();

        let result = row.checked_record(
            &recording,
            metric_observation(HookOutcome::Blocked, Some(&vendor_session_id)),
            &mut session_counts,
        );

        assert_eq!(
            result,
            Err(HookMetricsUpdateError::Duration(
                LatencyHistogramError::BucketCountOverflow { bucket: 0 }
            ))
        );
        assert_eq!(row, row_before);
        assert_eq!(session_counts, session_counts_before);
    }

    #[test]
    fn aggregate_rejects_an_observation_for_another_target_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let mut row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut session_counts,
        )
        .unwrap();
        let row_before = row.clone();
        let session_counts_before = session_counts.clone();
        let wrong_target = HookMetricObservation {
            agent: HookAgent::Codex,
            ..metric_observation(HookOutcome::Ok, Some(&vendor_session_id))
        };

        let result = row.checked_record(&recording, wrong_target, &mut session_counts);

        assert_eq!(result, Err(HookMetricsUpdateError::TargetChanged));
        assert_eq!(row, row_before);
        assert_eq!(session_counts, session_counts_before);
    }

    #[test]
    fn aggregate_rejects_an_observation_from_another_day_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let (mut row, mut session_counts) = {
            let recording = recording_observation(&mut state);
            let mut session_counts =
                session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
            let row = HookMetricsV1::new(
                &recording,
                metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
                &mut session_counts,
            )
            .unwrap();
            (row, session_counts)
        };
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 4, 10, 2, 11).unwrap());
        let later = state.observe_recording(completed_at).unwrap();
        let later = state.bind_recording_observation(later).unwrap();
        let row_before = row.clone();
        let session_counts_before = session_counts.clone();

        let result = row.checked_record(
            &later,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut session_counts,
        );

        assert_eq!(
            result,
            Err(HookMetricsUpdateError::DayChanged {
                row_day: row_before.day,
                observation_day: later.day(),
            })
        );
        assert_eq!(row, row_before);
        assert_eq!(session_counts, session_counts_before);
    }

    #[test]
    fn aggregate_rejects_another_identifier_epoch_without_mutation() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let (mut row, mut session_counts, completed_at) = {
            let recording = recording_observation(&mut state);
            let mut session_counts =
                session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
            let row = HookMetricsV1::new(
                &recording,
                metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
                &mut session_counts,
            )
            .unwrap();
            (row, session_counts, recording.completed_at())
        };
        state.reset_identifiers(row.day).unwrap();
        let recording = state.observe_recording(completed_at).unwrap();
        let recording = state.bind_recording_observation(recording).unwrap();
        let row_before = row.clone();
        let session_counts_before = session_counts.clone();

        let result = row.checked_record(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut session_counts,
        );

        assert_eq!(result, Err(HookMetricsUpdateError::IdentifierEpochChanged));
        assert_eq!(row, row_before);
        assert_eq!(session_counts, session_counts_before);
    }

    #[test]
    fn aggregate_rejects_a_tracker_for_another_hook_surface() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let mut pre_tool_tracker =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let mut post_tool_tracker =
            session_counts(&recording, HookAgent::Claude, HookSurface::PostToolUse);
        let mut pre_tool_row = HookMetricsV1::new(
            &recording,
            metric_observation(HookOutcome::Ok, Some(&vendor_session_id)),
            &mut pre_tool_tracker,
        )
        .unwrap();
        let post_tool_observation = HookMetricObservation {
            hook: HookSurface::PostToolUse,
            ..metric_observation(HookOutcome::Ok, Some(&vendor_session_id))
        };
        let _post_tool_row =
            HookMetricsV1::new(&recording, post_tool_observation, &mut post_tool_tracker).unwrap();
        let row_before = pre_tool_row.clone();
        let tracker_before = post_tool_tracker.clone();

        let result = pre_tool_row.checked_record(
            &recording,
            metric_observation(HookOutcome::Blocked, Some(&vendor_session_id)),
            &mut post_tool_tracker,
        );

        assert_eq!(result, Err(HookMetricsUpdateError::SessionTrackerChanged));
        assert_eq!(pre_tool_row, row_before);
        assert_eq!(post_tool_tracker, tracker_before);
    }

    #[test]
    fn invalid_plugin_counts_reject_the_first_observation_without_state_change() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let session_counts_before = session_counts.clone();
        let input = HookMetricObservation {
            plugins_attempted: 0,
            plugins_completed: 1,
            ..metric_observation(HookOutcome::Ok, None)
        };

        let result = HookMetricsV1::new(&recording, input, &mut session_counts);

        assert_eq!(
            result,
            Err(HookMetricsUpdateError::CompletedPluginsExceedAttempts {
                attempted: 0,
                completed: 1,
            })
        );
        assert_eq!(session_counts, session_counts_before);
    }

    #[test]
    fn invalid_plugin_counts_are_rejected_even_when_the_row_has_slack() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);
        let mut session_counts =
            session_counts(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let first = HookMetricObservation {
            plugins_attempted: 5,
            plugins_completed: 3,
            ..metric_observation(HookOutcome::Ok, None)
        };
        let mut row = HookMetricsV1::new(&recording, first, &mut session_counts).unwrap();
        let row_before = row.clone();
        let session_counts_before = session_counts.clone();
        let impossible = HookMetricObservation {
            plugins_attempted: 0,
            plugins_completed: 2,
            ..metric_observation(HookOutcome::Ok, None)
        };

        let result = row.checked_record(&recording, impossible, &mut session_counts);

        assert_eq!(
            result,
            Err(HookMetricsUpdateError::CompletedPluginsExceedAttempts {
                attempted: 0,
                completed: 2,
            })
        );
        assert_eq!(row, row_before);
        assert_eq!(session_counts, session_counts_before);
    }
}
