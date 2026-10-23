//! Disjoint private stores for cumulative telemetry rows.

use super::{
    extension_invocation::ExtensionInvocationAggregateStore, hook::HookAggregateStore,
    plugin_hook::PluginHookAggregateStore,
};
use crate::telemetry::schema::UtcDay;

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
/// without repeatedly borrowing a parent object.
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
    pub(in crate::telemetry::state) const fn hook_invocation_stores(
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
