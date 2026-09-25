//! Closed vocabulary for extension-invocation telemetry.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::super::agent::{HookAgent, SupportedAgent};
use crate::agents::Agent;

/// Agent with a structured skill-invocation signal in consent version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ExtensionInvocationAgent {
    Claude,
}

impl ExtensionInvocationAgent {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
        }
    }
}

impl From<ExtensionInvocationAgent> for SupportedAgent {
    fn from(agent: ExtensionInvocationAgent) -> Self {
        match agent {
            ExtensionInvocationAgent::Claude => Self::Claude,
        }
    }
}

impl From<ExtensionInvocationAgent> for HookAgent {
    fn from(agent: ExtensionInvocationAgent) -> Self {
        match agent {
            ExtensionInvocationAgent::Claude => Self::Claude,
        }
    }
}

impl TryFrom<Agent> for ExtensionInvocationAgent {
    type Error = UnsupportedExtensionInvocationAgent;

    fn try_from(agent: Agent) -> Result<Self, Self::Error> {
        match agent {
            Agent::Claude => Ok(Self::Claude),
            Agent::Codex
            | Agent::Copilot
            | Agent::Gemini
            | Agent::Goose
            | Agent::Kiro
            | Agent::OpenCode => Err(UnsupportedExtensionInvocationAgent { found: agent }),
        }
    }
}

/// Agent without a consent-version-1 structured skill-invocation signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct UnsupportedExtensionInvocationAgent {
    found: Agent,
}

impl fmt::Display for UnsupportedExtensionInvocationAgent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "agent {} has no supported skill-invocation signal in telemetry consent version 1",
            self.found.config_name()
        )
    }
}

impl std::error::Error for UnsupportedExtensionInvocationAgent {}

/// Phase contributed by one normalized skill-invocation observation.
///
/// A phase is producer vocabulary, not a standalone wire value. Each variant
/// increments the corresponding counter field in the aggregate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionInvocationPhase {
    Attempted,
    Completed,
    Failed,
}

impl ExtensionInvocationPhase {
    /// Return the aggregate counter field updated by this phase.
    #[must_use]
    pub(in crate::telemetry) const fn counter_name(self) -> &'static str {
        match self {
            Self::Attempted => "attempted",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// Identity exposure assigned to one extension-invocation aggregate bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ExtensionTargetScope {
    Public,
    Unnamed,
    Overflow,
}

impl ExtensionTargetScope {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unnamed => "unnamed",
            Self::Overflow => "overflow",
        }
    }
}

/// Fixed reason why a skill invocation cannot name a public target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum UnnamedExtensionReason {
    Ineligible,
    NotIndexed,
    AttributionUnavailable,
    Ambiguous,
    InvalidSignal,
}

impl UnnamedExtensionReason {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Ineligible => "ineligible",
            Self::NotIndexed => "not_indexed",
            Self::AttributionUnavailable => "attribution_unavailable",
            Self::Ambiguous => "ambiguous",
            Self::InvalidSignal => "invalid_signal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::schema::assert_contract_names_with_labels;

    #[test]
    fn extension_invocation_agents_round_trip_with_contract_names() {
        assert_contract_names_with_labels(
            &[(ExtensionInvocationAgent::Claude, "claude")],
            ExtensionInvocationAgent::as_str,
        );
    }

    #[test]
    fn extension_invocation_agent_name_matches_the_supported_agent_name() {
        let invocation_agent = ExtensionInvocationAgent::Claude;
        let supported_agent = SupportedAgent::from(invocation_agent);

        let invocation_name = serde_json::to_string(&invocation_agent).unwrap();
        let supported_name = serde_json::to_string(&supported_agent).unwrap();

        assert_eq!(invocation_name, supported_name);
    }

    #[test]
    fn extension_invocation_agent_name_matches_the_hook_agent_name() {
        let invocation_agent = ExtensionInvocationAgent::Claude;
        let hook_agent = HookAgent::from(invocation_agent);

        let invocation_name = serde_json::to_string(&invocation_agent).unwrap();
        let hook_name = serde_json::to_string(&hook_agent).unwrap();

        assert_eq!(invocation_name, hook_name);
    }

    #[test]
    fn only_claude_maps_to_the_version_one_invocation_agent() {
        for &agent in Agent::all() {
            let result = ExtensionInvocationAgent::try_from(agent);

            if agent == Agent::Claude {
                assert_eq!(result, Ok(ExtensionInvocationAgent::Claude));
            } else {
                assert_eq!(
                    result,
                    Err(UnsupportedExtensionInvocationAgent { found: agent })
                );
            }
        }
    }

    #[test]
    fn invocation_phases_name_their_aggregate_counter_fields() {
        let cases = [
            (ExtensionInvocationPhase::Attempted, "attempted"),
            (ExtensionInvocationPhase::Completed, "completed"),
            (ExtensionInvocationPhase::Failed, "failed"),
        ];

        for (phase, expected) in cases {
            assert_eq!(phase.counter_name(), expected);
        }
    }

    #[test]
    fn extension_target_scopes_round_trip_with_contract_names() {
        assert_contract_names_with_labels(
            &[
                (ExtensionTargetScope::Public, "public"),
                (ExtensionTargetScope::Unnamed, "unnamed"),
                (ExtensionTargetScope::Overflow, "overflow"),
            ],
            ExtensionTargetScope::as_str,
        );
    }

    #[test]
    fn unnamed_extension_reasons_round_trip_with_contract_names() {
        assert_contract_names_with_labels(
            &[
                (UnnamedExtensionReason::Ineligible, "ineligible"),
                (UnnamedExtensionReason::NotIndexed, "not_indexed"),
                (
                    UnnamedExtensionReason::AttributionUnavailable,
                    "attribution_unavailable",
                ),
                (UnnamedExtensionReason::Ambiguous, "ambiguous"),
                (UnnamedExtensionReason::InvalidSignal, "invalid_signal"),
            ],
            UnnamedExtensionReason::as_str,
        );
    }

    #[test]
    fn extension_invocation_vocabulary_rejects_unknown_wire_names() {
        let agent = serde_json::from_str::<ExtensionInvocationAgent>(r#""codex""#);
        let scope = serde_json::from_str::<ExtensionTargetScope>(r#""private""#);
        let reason = serde_json::from_str::<UnnamedExtensionReason>(r#""missing_attribution""#);

        assert!(agent.is_err());
        assert!(scope.is_err());
        assert!(reason.is_err());
    }
}
