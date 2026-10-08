//! Typed, content-free failures from invocation-wide aggregate staging.

use std::{error::Error, fmt};

use crate::telemetry::{
    schema::{
        EventId, ExtensionInvocationMetricsUpdateError, ExtensionInvocationPhase, HookAgent,
        HookMetricsUpdateError, HookSurface, PluginHookMetricsUpdateError, UtcDay,
    },
    state::{
        extension_invocation::ExtensionInvocationAdmissionError, hook::HookAggregateStoreError,
        plugin_hook::PluginHookAdmissionError,
    },
    storage::metrics::MetricSnapshotEditError,
};

use super::super::AggregateFamily;

/// Failure to stage one complete hook invocation across aggregate families.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum AggregateRecordingError {
    UnsupportedExtensionAgent(HookAgent),
    ExtensionPhaseDoesNotMatchHook {
        hook: HookSurface,
        phase: ExtensionInvocationPhase,
    },
    HookState(HookAggregateStoreError),
    PluginHookState(PluginHookAdmissionError),
    ExtensionState(ExtensionInvocationAdmissionError),
    SnapshotDayMismatch {
        snapshot_day: UtcDay,
        recording_day: UtcDay,
    },
    HookRow(HookMetricsUpdateError),
    PluginHookRow(PluginHookMetricsUpdateError),
    ExtensionRow(ExtensionInvocationMetricsUpdateError),
    Snapshot(MetricSnapshotEditError),
    SnapshotRowKindChanged {
        event_id: EventId,
        expected: AggregateFamily,
    },
    /// A coordinator path swallowed a selection error instead of propagating it.
    PoisonedStageInvariant,
}

impl fmt::Display for AggregateRecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedExtensionAgent(agent) => write!(
                formatter,
                "hook agent {agent} cannot contribute extension_invocation_metrics telemetry"
            ),
            Self::ExtensionPhaseDoesNotMatchHook { hook, phase } => write!(
                formatter,
                "extension-invocation phase {phase} cannot come from hook surface {hook}"
            ),
            Self::HookState(_) => formatter.write_str("failed to select hook aggregate state"),
            Self::PluginHookState(_) => {
                formatter.write_str("failed to select plugin-hook aggregate state")
            }
            Self::ExtensionState(_) => {
                formatter.write_str("failed to select extension-invocation aggregate state")
            }
            Self::SnapshotDayMismatch {
                snapshot_day,
                recording_day,
            } => write!(
                formatter,
                "aggregate metric snapshot belongs to {snapshot_day}, not recording day {recording_day}"
            ),
            Self::HookRow(_) => formatter.write_str("failed to update hook aggregate row"),
            Self::PluginHookRow(_) => {
                formatter.write_str("failed to update plugin-hook aggregate row")
            }
            Self::ExtensionRow(_) => {
                formatter.write_str("failed to update extension-invocation aggregate row")
            }
            Self::Snapshot(_) => formatter.write_str("failed to edit aggregate metric snapshot"),
            Self::SnapshotRowKindChanged { event_id, expected } => write!(
                formatter,
                "aggregate row {event_id} is not the expected {expected} row"
            ),
            Self::PoisonedStageInvariant => formatter.write_str(
                "aggregate coordinator reached commit after swallowing a selection error",
            ),
        }
    }
}

impl Error for AggregateRecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::HookState(error) => Some(error),
            Self::PluginHookState(error) => Some(error),
            Self::ExtensionState(error) => Some(error),
            Self::HookRow(error) => Some(error),
            Self::PluginHookRow(error) => Some(error),
            Self::ExtensionRow(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::UnsupportedExtensionAgent(_)
            | Self::ExtensionPhaseDoesNotMatchHook { .. }
            | Self::SnapshotDayMismatch { .. }
            | Self::SnapshotRowKindChanged { .. }
            | Self::PoisonedStageInvariant => None,
        }
    }
}
