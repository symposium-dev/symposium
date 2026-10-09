//! Disjoint private stores for cumulative telemetry rows.

mod persistence;
pub(in crate::telemetry) mod recording;

pub(super) use persistence::{
    AggregatePersistenceContext, AggregateStateInvariantError, AggregateStateRef, RawAggregateState,
};

use std::fmt;

use super::{
    extension_invocation::ExtensionInvocationAggregateStore, hook::HookAggregateStore,
    plugin_hook::PluginHookAggregateStore,
};
use crate::telemetry::schema::UtcDay;

use super::DayBeforeCurrent;

/// Aggregate row family named by coordinator and persistence diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum AggregateFamily {
    Hook,
    PluginHook,
    ExtensionInvocation,
}

impl fmt::Display for AggregateFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hook => "hook_metrics",
            Self::PluginHook => "plugin_hook_metrics",
            Self::ExtensionInvocation => "extension_invocation_metrics",
        })
    }
}

/// Result of consuming one aggregate private-state stage.
///
/// Committing remains infallible because one recording operation applies
/// several stores sequentially. A stage poisoned by failed selection instead
/// reports that it discarded every staged edit.
#[must_use = "aggregate stage commit outcomes must be observed"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum StageCommit {
    /// Every staged edit was applied to its destination store.
    Applied,
    /// A selection failed, so every staged edit was discarded.
    DiscardedPoisoned,
}

/// Private state owned by the three aggregate row families.
///
/// One hook observation may update all three families. Keeping the stores as
/// separate fields lets the recording layer hold their stages at the same time
/// without repeatedly borrowing a parent object. The coordinator opens every
/// stage for each invocation, even when a family has no row contribution, so
/// their monotonic days advance together. A successful staged unit commits all
/// three stores; any staging or row-update failure drops all three.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(in crate::telemetry) struct AggregateState {
    hook: HookAggregateStore,
    plugin_hook: PluginHookAggregateStore,
    extension_invocation: ExtensionInvocationAggregateStore,
}

impl AggregateState {
    #[must_use]
    pub(in crate::telemetry) const fn new(day: UtcDay) -> Self {
        Self {
            hook: HookAggregateStore::new(day),
            plugin_hook: PluginHookAggregateStore::new(day),
            extension_invocation: ExtensionInvocationAggregateStore::new(day),
        }
    }

    /// Advance every daily aggregate store as one preflighted unit.
    ///
    /// The caller validates all three current days before any store changes.
    /// Once that succeeds, applying the day is infallible: a later day clears
    /// closed-day entries and resets each named family's public-row spend.
    pub(super) fn advance_day(&mut self, observed: UtcDay) -> Result<(), DayBeforeCurrent> {
        for current in [
            self.hook.day(),
            self.plugin_hook.day(),
            self.extension_invocation.day(),
        ] {
            if observed < current {
                return Err(DayBeforeCurrent { current, observed });
            }
        }

        self.hook.advance_day_after_preflight(observed);
        self.plugin_hook.advance_day_after_preflight(observed);
        self.extension_invocation
            .advance_day_after_preflight(observed);
        Ok(())
    }

    /// Remove entries derived under the previous identifier epoch.
    ///
    /// Public-row spend deliberately survives so changing epochs cannot grant
    /// another daily allowance.
    pub(super) fn reset_identifier_epoch(&mut self) {
        self.hook.reset_identifier_epoch();
        self.plugin_hook.reset_identifier_epoch();
        self.extension_invocation.reset_identifier_epoch();
    }

    /// Remove private state paired with cleared aggregate snapshots.
    pub(super) fn clear(&mut self) {
        self.hook.clear();
        self.plugin_hook.clear();
        self.extension_invocation.clear();
    }

    /// Borrow the three stores one hook observation may update.
    #[must_use]
    const fn hook_invocation_stores(
        &mut self,
    ) -> (
        &mut HookAggregateStore,
        &mut PluginHookAggregateStore,
        &mut ExtensionInvocationAggregateStore,
    ) {
        (
            &mut self.hook,
            &mut self.plugin_hook,
            &mut self.extension_invocation,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{NaiveDate, TimeZone as _, Utc};

    use super::*;
    use crate::telemetry::{
        schema::{
            ExtensionInvocationAttribution, ExtensionInvocationPhase, HookAgent, HookOutcome,
            HookSurface, PluginHookAttempt, PluginHookAttribution, UtcSecond,
        },
        state::{
            IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1, decode, encode, recording_observation,
        },
        storage::metrics::MetricSnapshot,
    };
    use recording::{
        ExtensionInvocationObservation, HookInvocationMetricObservation,
        PluginHookInvocationObservation,
    };

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    fn completed_at(month: u32, day: u32) -> UtcSecond {
        UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, month, day, 12, 0, 0).unwrap())
    }

    fn populated_state() -> TelemetryStateV1 {
        let mut state = TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
        let observation = state.observe_recording(completed_at(8, 3)).unwrap();
        let aggregates = {
            let recording = state.bind_recording_observation(observation).unwrap();
            let plugin = serde_json::from_value(serde_json::json!({
                "source": "symposium-recommendations",
                "name": "lifecycle-test",
            }))
            .unwrap();
            let observation = HookInvocationMetricObservation {
                agent: HookAgent::Claude,
                hook: HookSurface::PreToolUse,
                outcome: HookOutcome::Ok,
                duration: Duration::from_millis(7),
                vendor_session_id: None,
                plugin_attempts: vec![PluginHookInvocationObservation {
                    attribution: PluginHookAttribution::Public(plugin),
                    terminal: Some(PluginHookAttempt::NotExecuted {
                        prepare_duration: Duration::from_millis(2),
                    }),
                }],
                extension: Some(ExtensionInvocationObservation {
                    attribution: ExtensionInvocationAttribution::Public(
                        serde_json::from_value(serde_json::json!({
                            "target": {
                                "type": "skill",
                                "source": "symposium-recommendations",
                                "name": "lifecycle-test",
                            },
                            "path": [{ "type": "not" }],
                        }))
                        .unwrap(),
                    ),
                    phase: ExtensionInvocationPhase::Attempted,
                }),
            };
            let mut aggregates = AggregateState::new(recording.day());
            let _staged = aggregates
                .stage_hook_invocation(
                    &recording,
                    MetricSnapshot::empty(recording.day()),
                    observation,
                )
                .unwrap();
            aggregates
        };
        state.aggregates = aggregates;
        state
    }

    fn assert_round_trip(state: &TelemetryStateV1) {
        let encoded = encode(state).unwrap();
        let decoded = decode(encoded.as_bytes()).unwrap();

        // Keep identity keys out of assertion diagnostics.
        assert!(decoded == *state, "decoded private state differs");
        assert_eq!(encode(&decoded).unwrap(), encoded);
    }

    #[test]
    fn aggregate_families_use_their_wire_labels() {
        let cases = [
            (AggregateFamily::Hook, "hook_metrics"),
            (AggregateFamily::PluginHook, "plugin_hook_metrics"),
            (
                AggregateFamily::ExtensionInvocation,
                "extension_invocation_metrics",
            ),
        ];

        for (family, label) in cases {
            assert_eq!(family.to_string(), label);
        }
    }

    #[test]
    fn all_three_aggregate_stages_can_remain_live_together() {
        let mut state: TelemetryStateV1 =
            TelemetryStateV1::decode_for_test(IDENTIFIER_WINDOW_TEST_STATE);
        let recording = recording_observation(&mut state);
        let recovery = MetricSnapshot::empty(recording.day()).recovery_index();
        let mut aggregates = AggregateState::new(recording.day());

        let (hook, plugin_hook, extension_invocation) = aggregates.hook_invocation_stores();
        let hook_stage = hook.stage(&recording).unwrap();
        let plugin_hook_stage = plugin_hook.stage(&recovery, &recording).unwrap();
        let extension_invocation_stage = extension_invocation.stage(&recovery, &recording).unwrap();

        assert_eq!(hook_stage.commit(), StageCommit::Applied);
        assert_eq!(plugin_hook_stage.commit(), StageCommit::Applied);
        assert_eq!(extension_invocation_stage.commit(), StageCommit::Applied);
    }

    #[test]
    fn same_day_low_volume_recording_keeps_aggregate_state_and_round_trips() {
        let mut state = populated_state();
        let before = state.aggregates.clone();

        let _ = state.observe_recording(completed_at(8, 3)).unwrap();

        assert!(state.aggregates == before);
        assert_round_trip(&state);
    }

    #[test]
    fn new_day_low_volume_recording_advances_and_clears_every_store() {
        let mut state = populated_state();

        let _ = state.observe_recording(completed_at(8, 4)).unwrap();

        assert!(state.aggregates == AggregateState::new(day(4)));
        assert_round_trip(&state);
    }

    #[test]
    fn identifier_window_rollover_clears_the_previous_epoch() {
        let mut state = populated_state();
        let rollover_day = completed_at(9, 2).day();

        let observation = state.observe_recording(completed_at(9, 2)).unwrap();

        assert!(matches!(
            observation.identifier_window,
            crate::telemetry::state::lifecycle::IdentifierWindowUpdate::Advanced { .. }
        ));
        assert!(state.aggregates == AggregateState::new(rollover_day));
        assert_round_trip(&state);
    }

    #[test]
    fn identifier_reset_clears_entries_without_resetting_same_day_spend() {
        let mut state = populated_state();
        let plugin_spent = state.aggregates.plugin_hook.public_rows_spent();
        let extension_spent = state.aggregates.extension_invocation.public_rows_spent();

        assert_eq!(plugin_spent, 1);
        assert_eq!(extension_spent, 1);

        state.reset_identifiers(day(3)).unwrap();

        assert_eq!(state.aggregates.hook.persistence_entries().len(), 0);
        assert_eq!(state.aggregates.plugin_hook.persistence_entries().len(), 0);
        assert_eq!(
            state
                .aggregates
                .extension_invocation
                .persistence_entries()
                .len(),
            0
        );
        assert_eq!(
            state.aggregates.plugin_hook.public_rows_spent(),
            plugin_spent
        );
        assert_eq!(
            state.aggregates.extension_invocation.public_rows_spent(),
            extension_spent
        );
        assert_round_trip(&state);
    }

    #[test]
    fn clear_removes_aggregate_state_and_the_stopped_event_day() {
        let mut state = populated_state();
        state.stop_event_recording(day(3));

        assert_eq!(state.aggregates.plugin_hook.public_rows_spent(), 1);
        assert_eq!(state.aggregates.extension_invocation.public_rows_spent(), 1);

        state.clear_recorded_data();

        assert!(state.aggregates == AggregateState::new(day(3)));
        assert_eq!(state.aggregates.plugin_hook.public_rows_spent(), 0);
        assert_eq!(state.aggregates.extension_invocation.public_rows_spent(), 0);
        assert_eq!(state.storage_limit_day(), None);
        assert_round_trip(&state);
    }

    #[test]
    fn aggregate_day_preflight_prevents_partial_advancement() {
        let mut aggregates = AggregateState::new(day(3));
        aggregates.extension_invocation = ExtensionInvocationAggregateStore::new(day(4));
        let before = aggregates.clone();

        let result = aggregates.advance_day(day(3));

        assert_eq!(
            result,
            Err(DayBeforeCurrent {
                current: day(4),
                observed: day(3),
            })
        );
        assert!(aggregates == before);
    }
}
