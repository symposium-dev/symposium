//! Three-family private-state staging for one hook invocation.

use crate::telemetry::{
    schema::{ExtensionInvocationMetricObservation, HookMetricObservation},
    state::{
        BoundRecordingObservation, StageCommit,
        extension_invocation::{
            ExtensionInvocationAggregateStage, ExtensionInvocationAggregateStore,
        },
        hook::{HookAggregateStage, HookAggregateStore},
        plugin_hook::{PluginHookAggregateStage, PluginHookAggregateStore},
    },
    storage::metrics::{AggregateRecoveryIndex, MetricSnapshot},
};

use super::{
    AggregateRecordingError, NormalizedHookInvocation,
    rows::{record_extension_row, record_hook_row, record_plugin_hook_row},
};

type AggregateStores<'a> = (
    &'a mut HookAggregateStore,
    &'a mut PluginHookAggregateStore,
    &'a mut ExtensionInvocationAggregateStore,
);

/// Copy-on-write private stages for one invocation-wide snapshot edit.
pub(super) struct InvocationStages<'store, 'context, 'identity> {
    recording: &'context BoundRecordingObservation<'identity>,
    recovery: &'context AggregateRecoveryIndex,
    hook: HookAggregateStage<'store, 'context, 'identity>,
    plugin_hook: PluginHookAggregateStage<'store, 'context, 'identity>,
    extension: ExtensionInvocationAggregateStage<'store, 'context, 'identity>,
}

impl<'store, 'context, 'identity> InvocationStages<'store, 'context, 'identity> {
    /// Open all three private stages after checking the snapshot day.
    pub(super) fn open(
        stores: AggregateStores<'store>,
        recovery: &'context AggregateRecoveryIndex,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<Self, AggregateRecordingError> {
        if recovery.day() != recording.day() {
            return Err(AggregateRecordingError::SnapshotDayMismatch {
                snapshot_day: recovery.day(),
                recording_day: recording.day(),
            });
        }

        let (hook_store, plugin_hook_store, extension_store) = stores;
        let hook = hook_store
            .stage(recording)
            .map_err(AggregateRecordingError::HookState)?;
        let plugin_hook = plugin_hook_store
            .stage(recovery, recording)
            .map_err(AggregateRecordingError::PluginHookState)?;
        let extension = extension_store
            .stage(recovery, recording)
            .map_err(AggregateRecordingError::ExtensionState)?;

        Ok(Self {
            recording,
            recovery,
            hook,
            plugin_hook,
            extension,
        })
    }

    /// Apply exactly one complete invocation to this staged unit.
    pub(super) fn apply(
        &mut self,
        snapshot: &mut MetricSnapshot,
        invocation: NormalizedHookInvocation<'_>,
    ) -> Result<(), AggregateRecordingError> {
        let NormalizedHookInvocation {
            target,
            outcome,
            duration,
            plugins_attempted,
            plugins_completed,
            plugin_attempts,
            extension,
        } = invocation;

        record_hook_row(
            snapshot,
            self.recovery,
            self.recording,
            &mut self.hook,
            HookMetricObservation {
                agent: target.agent,
                hook: target.hook,
                outcome,
                plugins_attempted,
                plugins_completed,
                duration,
                vendor_session_id: target.vendor_session_id,
            },
        )?;

        for plugin in plugin_attempts {
            let Some(attempt) = plugin.terminal else {
                continue;
            };
            record_plugin_hook_row(
                snapshot,
                self.recording,
                &mut self.plugin_hook,
                target,
                plugin.attribution,
                attempt,
            )?;
        }

        if let Some((extension_agent, extension)) = extension {
            record_extension_row(
                snapshot,
                self.recording,
                &mut self.extension,
                extension_agent,
                extension.attribution,
                ExtensionInvocationMetricObservation {
                    phase: extension.phase,
                    vendor_session_id: target.vendor_session_id,
                },
            )?;
        }

        Ok(())
    }

    /// Commit all three private stages after one shared poison preflight.
    pub(super) fn commit(self) -> Result<(), AggregateRecordingError> {
        let poisoned = self.hook.is_poisoned()
            || self.plugin_hook.is_poisoned()
            || self.extension.is_poisoned();
        debug_assert!(
            !poisoned,
            "BUG: aggregate coordinator swallowed a selection error"
        );
        if poisoned {
            // Every selection error in `apply` is propagated with `?`.
            // Reaching this branch means a future path swallowed one.
            return Err(AggregateRecordingError::PoisonedStageInvariant);
        }

        // Evaluate every infallible commit outside `debug_assert!`: debug
        // assertions disappear in release builds, but these state changes
        // must not.
        let hook_commit = self.hook.commit();
        let plugin_hook_commit = self.plugin_hook.commit();
        let extension_commit = self.extension.commit();
        debug_assert_eq!(hook_commit, StageCommit::Applied);
        debug_assert_eq!(plugin_hook_commit, StageCommit::Applied);
        debug_assert_eq!(extension_commit, StageCommit::Applied);

        Ok(())
    }
}
