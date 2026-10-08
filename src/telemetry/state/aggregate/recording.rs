//! Complete inputs for invocation-wide aggregate staging.
//!
//! One value owns every input needed by the hook, plugin-hook, and optional
//! extension-invocation rows. Plugin totals are derived from that collection,
//! so callers cannot claim counts that disagree with the plugin observations
//! supplied to the coordinator.

use std::time::Duration;

use crate::telemetry::schema::{
    ExtensionInvocationAgent, ExtensionInvocationAttribution, ExtensionInvocationPhase, HookAgent,
    HookOutcome, HookSurface, PluginHookAttempt, PluginHookAttribution, VendorSessionId,
};
use crate::telemetry::{state::BoundRecordingObservation, storage::metrics::MetricSnapshot};

use super::AggregateState;

mod error;
mod rows;
mod stages;

pub(in crate::telemetry) use error::AggregateRecordingError;
use stages::InvocationStages;

/// Every aggregate input observed for one completed top-level hook invocation.
///
/// Plugin counts are absent deliberately. They are derived from
/// `plugin_attempts`, making a claimed count that disagrees with the supplied
/// observations unrepresentable at this boundary.
// Intentionally omit `Debug`: this value borrows a raw vendor session id.
pub(in crate::telemetry) struct HookInvocationMetricObservation<'a> {
    pub(in crate::telemetry) agent: HookAgent,
    pub(in crate::telemetry) hook: HookSurface,
    pub(in crate::telemetry) outcome: HookOutcome,
    pub(in crate::telemetry) duration: Duration,
    pub(in crate::telemetry) vendor_session_id: Option<&'a VendorSessionId>,
    pub(in crate::telemetry) plugin_attempts: Vec<PluginHookInvocationObservation>,
    pub(in crate::telemetry) extension: Option<ExtensionInvocationObservation>,
}

/// One plugin whose preparation began during a top-level hook invocation.
///
/// A missing terminal result contributes only to the hook row's attempted
/// count. A terminal result additionally contributes to the completed count
/// and will update one plugin-hook row.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct PluginHookInvocationObservation {
    pub(in crate::telemetry) attribution: PluginHookAttribution,
    pub(in crate::telemetry) terminal: Option<PluginHookAttempt>,
}

/// Optional structured skill signal carried by the same hook invocation.
///
/// This coordinator accepts `attempted` from `pre_tool_use` and `completed`
/// from `post_tool_use`. The targeted failure signal has no top-level hook row
/// and will use its own recording entry point.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionInvocationObservation {
    pub(in crate::telemetry) attribution: ExtensionInvocationAttribution,
    pub(in crate::telemetry) phase: ExtensionInvocationPhase,
}

/// Shared top-level identity for every row produced by one hook invocation.
#[derive(Clone, Copy)]
struct HookInvocationTarget<'a> {
    agent: HookAgent,
    hook: HookSurface,
    vendor_session_id: Option<&'a VendorSessionId>,
}

/// Validated invocation inputs with their derived plugin totals.
///
/// Construction is the only place that interprets the complete observation,
/// so row staging cannot receive independently supplied plugin counts or an
/// extension phase from the wrong hook surface.
struct NormalizedHookInvocation<'a> {
    target: HookInvocationTarget<'a>,
    outcome: HookOutcome,
    duration: Duration,
    plugins_attempted: u64,
    plugins_completed: u64,
    plugin_attempts: Vec<PluginHookInvocationObservation>,
    extension: Option<(ExtensionInvocationAgent, ExtensionInvocationObservation)>,
}

impl<'a> NormalizedHookInvocation<'a> {
    fn new(
        observation: HookInvocationMetricObservation<'a>,
    ) -> Result<Self, AggregateRecordingError> {
        let HookInvocationMetricObservation {
            agent,
            hook,
            outcome,
            duration,
            vendor_session_id,
            plugin_attempts,
            extension,
        } = observation;
        let extension = extension
            .map(|extension| {
                let extension_agent = extension_agent(agent)?;
                ensure_extension_phase(hook, extension.phase)?;
                Ok((extension_agent, extension))
            })
            .transpose()?;

        Ok(Self {
            target: HookInvocationTarget {
                agent,
                hook,
                vendor_session_id,
            },
            outcome,
            duration,
            plugins_attempted: usize_to_u64_saturating(plugin_attempts.len()),
            plugins_completed: usize_to_u64_saturating(
                plugin_attempts
                    .iter()
                    .filter(|observation| observation.terminal.is_some())
                    .count(),
            ),
            plugin_attempts,
            extension,
        })
    }
}

/// Aggregate snapshot staged in memory but not anchored by persisted state.
///
/// This type deliberately exposes no preparation or publication method. The
/// persistence transaction will consume it only after replacing the owned
/// per-operation private state that selected its identifiers.
#[must_use = "an aggregate recording must be persisted or explicitly dropped"]
#[derive(Debug)]
pub(in crate::telemetry) struct StagedAggregateRecording {
    snapshot: MetricSnapshot,
}

impl AggregateState {
    /// Stage one complete hook invocation across every aggregate family.
    ///
    /// All three private stores are staged even when the observation has no
    /// plugin or extension contribution. This keeps their monotonic open days
    /// aligned. The supplied snapshot is owned by the operation and is dropped
    /// with every private overlay when any selection or row update fails.
    ///
    /// # Errors
    ///
    /// Returns the first typed selection, row-update, or snapshot-edit error.
    /// No private aggregate state is committed and no staged snapshot is
    /// returned on failure.
    pub(in crate::telemetry) fn stage_hook_invocation(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        mut snapshot: MetricSnapshot,
        observation: HookInvocationMetricObservation<'_>,
    ) -> Result<StagedAggregateRecording, AggregateRecordingError> {
        let invocation = NormalizedHookInvocation::new(observation)?;
        let recovery = snapshot.recovery_index();
        let mut stages =
            InvocationStages::open(self.hook_invocation_stores(), &recovery, recording)?;
        stages.apply(&mut snapshot, invocation)?;
        stages.commit()?;

        Ok(StagedAggregateRecording { snapshot })
    }
}

const fn extension_agent(
    agent: HookAgent,
) -> Result<ExtensionInvocationAgent, AggregateRecordingError> {
    match agent {
        HookAgent::Claude => Ok(ExtensionInvocationAgent::Claude),
        HookAgent::Antigravity | HookAgent::Codex | HookAgent::Copilot | HookAgent::Kiro => {
            Err(AggregateRecordingError::UnsupportedExtensionAgent(agent))
        }
    }
}

const fn ensure_extension_phase(
    hook: HookSurface,
    phase: ExtensionInvocationPhase,
) -> Result<(), AggregateRecordingError> {
    match (hook, phase) {
        (HookSurface::PreToolUse, ExtensionInvocationPhase::Attempted)
        | (HookSurface::PostToolUse, ExtensionInvocationPhase::Completed) => Ok(()),
        _ => Err(AggregateRecordingError::ExtensionPhaseDoesNotMatchHook { hook, phase }),
    }
}

fn usize_to_u64_saturating(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
