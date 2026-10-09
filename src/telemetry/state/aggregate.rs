//! Disjoint private stores for cumulative telemetry rows.

mod persistence;
pub(in crate::telemetry) mod recording;

use std::fmt;

use super::{
    extension_invocation::ExtensionInvocationAggregateStore, hook::HookAggregateStore,
    plugin_hook::PluginHookAggregateStore,
};
use crate::telemetry::schema::UtcDay;

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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    use super::*;
    use crate::telemetry::{
        state::{IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1, recording_observation},
        storage::metrics::MetricSnapshot,
    };

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
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
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
}
