//! Typed, content-free failures from aggregate observation normalization.

use std::fmt;

use crate::telemetry::schema::{ExtensionInvocationPhase, HookAgent, HookSurface};

/// Failure to normalize one complete hook invocation for aggregate staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum AggregateRecordingError {
    UnsupportedExtensionAgent(HookAgent),
    ExtensionPhaseDoesNotMatchHook {
        hook: HookSurface,
        phase: ExtensionInvocationPhase,
    },
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
        }
    }
}

impl std::error::Error for AggregateRecordingError {}
