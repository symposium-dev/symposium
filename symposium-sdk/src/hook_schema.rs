//! Native agent hook protocols and translations to the portable [`crate::hook`] types.

use anyhow::Result;
use std::{any::Any, fmt::Debug};

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod goose;
pub mod kiro;
pub mod opencode;
pub mod symposium;

/// Agents and events shared by native protocol adapters and plugin manifests.
pub use crate::hook::{HookAgent, HookEvent};

/// Native denial responses require a reason, while the portable output makes
/// context optional. Use that context as the reason when one was supplied.
fn denial_reason(context: Option<&str>) -> String {
    context
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("Tool call denied by a Symposium hook.")
        .to_owned()
}

/// The handler that speaks `agent`'s wire format for `event`, if it has one.
pub fn agent_event(agent: HookAgent, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
    match agent {
        HookAgent::Antigravity => antigravity::Antigravity.event(event),
        HookAgent::Claude => claude::ClaudeCode.event(event),
        HookAgent::Codex => codex::Codex.event(event),
        HookAgent::Copilot => copilot::Copilot.event(event),
        HookAgent::Goose => goose::Goose.event(event),
        HookAgent::Kiro => kiro::Kiro.event(event),
        HookAgent::OpenCode => opencode::OpenCode.event(event),
    }
}

/// Represents the data sent *from* an agent *to* a hook.
pub trait AgentHookInput: Debug {
    /// Parse an incoming JSON payload string into a concrete payload struct.
    fn parse_input(payload: &str) -> Result<Self>
    where
        Self: Sized;
    /// Convert this payload into the canonical symposium input event.
    fn to_symposium(&self) -> symposium::InputEvent;
    /// Convert a canonical symposium input event into this agent's payload.
    fn from_symposium(event: &symposium::InputEvent) -> Self
    where
        Self: Sized;
    /// Convert this payload into a JSON string for forwarding to plugins.
    fn to_string(&self) -> Result<String>;

    /// Whether this payload is an occurrence of the symposium event it maps
    /// to. An agent's native event can be broader than the canonical one
    /// (Antigravity's `PreInvocation` fires before every model call but stands
    /// in for `user-prompt-submit`), and a payload that is not such an
    /// occurrence is answered with the agent's neutral output and dispatched
    /// to nothing.
    fn should_dispatch(&self) -> bool {
        true
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

/// Represents the data sent *from* a hook *to* an agent.
pub trait AgentHookOutput: Debug {
    /// Parse raw stdout bytes from a hook handler into a concrete output struct.
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized;
    /// Convert a canonical symposium output event into this agent's output.
    fn from_symposium(event: &symposium::OutputEvent) -> Self
    where
        Self: Sized;
    /// Convert this agent's output into a canonical symposium output event.
    fn to_symposium(&self) -> symposium::OutputEvent;
    /// Convert this output into a JSON value to return to the agent.
    fn to_hook_output(&self) -> serde_json::Value;

    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

/// Represents the "handler" for a specific kind of hook event
/// (e.g., a `PreToolUse` event coming from `claude`),
/// capable of parsing the native payloads and outputs
/// as well as converting from symposium types.
pub trait AgentHookEvent {
    type Input: AgentHookInput;
    type Output: AgentHookOutput;

    /// Parse an incoming JSON payload string into a concrete payload struct.
    fn parse_input(&self, payload: &str) -> anyhow::Result<Self::Input> {
        Self::Input::parse_input(payload)
    }
    /// Parse raw stdout bytes from a hook handler into a concrete output struct.
    fn parse_output(&self, output: &[u8]) -> anyhow::Result<Self::Output> {
        Self::Output::parse_output(output)
    }
    /// Convert a canonical symposium output event into this agent's output.
    fn translate_output(&self, output: &symposium::OutputEvent) -> Self::Output {
        Self::Output::from_symposium(output)
    }

    /// Serialize the final output Value to bytes for stdout.
    /// Default: JSON. Override for agents with different output formats (e.g., Kiro plain text).
    fn serialize_output(&self, output: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(output).unwrap()
    }
}

/// Represents an agent that can handle hook events.
pub trait Agent {
    fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>>;
}

/// Erased version of [`AgentHookEvent`]. Represents the "handler" for a
/// specific kind of hook event (e.g., a `PreToolUse` event coming from `claude`),
/// capable of parsing the native payloads and outputs as well as
/// converting from symposium types.
pub trait ErasedAgentHookEvent {
    /// Parse an incoming JSON payload into a boxed `AgentHookPayload`.
    fn parse_input(&self, payload: &str) -> Result<Box<dyn AgentHookInput>>;

    /// Parses a stdout into a boxed `AgentHookOutput`.
    fn parse_output(&self, output: &[u8]) -> Result<Box<dyn AgentHookOutput>>;

    /// Convert a canonical symposium output event into a boxed agent output.
    fn translate_output(&self, output: &symposium::OutputEvent) -> Box<dyn AgentHookOutput>;

    /// Convert a canonical symposium input event into a boxed agent payload.
    fn translate_input(&self, input: &symposium::InputEvent) -> Box<dyn AgentHookInput>;

    /// Serialize the final accumulated output (as JSON Value) to bytes for stdout.
    /// Most agents emit JSON; Kiro emits plain text.
    fn serialize_output(&self, output: &serde_json::Value) -> Vec<u8>;
}

struct ErasedAgentHookEventImpl<E: AgentHookEvent + 'static>(E);

impl<E> ErasedAgentHookEvent for ErasedAgentHookEventImpl<E>
where
    E: AgentHookEvent + 'static,
    E::Input: AgentHookInput + 'static,
    E::Output: AgentHookOutput + 'static,
{
    fn parse_input(&self, payload: &str) -> Result<Box<dyn AgentHookInput>> {
        let p = self.0.parse_input(payload)?;
        Ok(Box::new(p))
    }

    fn parse_output(&self, output: &[u8]) -> Result<Box<dyn AgentHookOutput>> {
        let o = self.0.parse_output(output)?;
        Ok(Box::new(o))
    }
    fn translate_output(&self, output: &symposium::OutputEvent) -> Box<dyn AgentHookOutput> {
        Box::new(self.0.translate_output(output))
    }

    fn translate_input(&self, input: &symposium::InputEvent) -> Box<dyn AgentHookInput> {
        Box::new(E::Input::from_symposium(input))
    }

    fn serialize_output(&self, output: &serde_json::Value) -> Vec<u8> {
        self.0.serialize_output(output)
    }
}

/// Helper to erase a concrete `AgentHookEvent` into a trait object.
pub fn erase_agent_hook_event<E>(e: E) -> Box<dyn ErasedAgentHookEvent>
where
    E: AgentHookEvent + 'static,
    E::Input: AgentHookInput + 'static,
    E::Output: AgentHookOutput + 'static,
{
    Box::new(ErasedAgentHookEventImpl(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::{Decision, Output, PreToolUseOutput};
    use serde_json::json;

    #[test]
    fn claude_and_codex_preserve_portable_pre_tool_outputs() {
        let cases = [
            (PreToolUseOutput::default(), json!({})),
            (
                PreToolUseOutput::context("Use the staging environment"),
                json!({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": "Use the staging environment"
                }}),
            ),
            (
                PreToolUseOutput::new(
                    Decision::Allow,
                    Some("Use a harmless command".into()),
                    Some(json!({"command": "pwd"})),
                ),
                json!({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "additionalContext": "Use a harmless command",
                    "updatedInput": {"command": "pwd"}
                }}),
            ),
            (
                PreToolUseOutput::new(Decision::Deny, Some("Protected directory".into()), None),
                json!({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "Protected directory",
                    "additionalContext": "Protected directory"
                }}),
            ),
        ];

        for agent in [HookAgent::Claude, HookAgent::Codex] {
            let adapter = agent_event(agent, HookEvent::PreToolUse).unwrap();
            for (output, expected_wire) in &cases {
                let canonical = Output::PreToolUse(output.clone());
                let wire = adapter.translate_output(&canonical).to_hook_output();
                assert_eq!(&wire, expected_wire, "agent: {agent:?}");
                let roundtrip = adapter
                    .parse_output(&adapter.serialize_output(&wire))
                    .unwrap()
                    .to_symposium();
                assert_eq!(
                    serde_json::to_value(roundtrip).unwrap(),
                    serde_json::to_value(canonical).unwrap(),
                    "agent: {agent:?}"
                );
            }
        }
    }

    #[test]
    fn denials_without_context_still_have_a_reason() {
        for agent in [HookAgent::Claude, HookAgent::Codex, HookAgent::Copilot] {
            let adapter = agent_event(agent, HookEvent::PreToolUse).unwrap();
            for context in [None, Some(" \n".into())] {
                let wire = adapter
                    .translate_output(&Output::PreToolUse(PreToolUseOutput::new(
                        Decision::Deny,
                        context,
                        None,
                    )))
                    .to_hook_output();
                let fields = wire.get("hookSpecificOutput").unwrap_or(&wire);
                assert_eq!(fields["permissionDecision"], "deny");
                assert!(
                    !fields["permissionDecisionReason"]
                        .as_str()
                        .unwrap()
                        .trim()
                        .is_empty()
                );
                let Output::PreToolUse(roundtrip) = adapter
                    .parse_output(&adapter.serialize_output(&wire))
                    .unwrap()
                    .to_symposium()
                else {
                    panic!("wrong event type")
                };
                assert_eq!(roundtrip.decision, Decision::Deny);
            }
        }
    }

    #[test]
    fn denial_takes_precedence_over_input_replacement() {
        for agent in [HookAgent::Claude, HookAgent::Codex] {
            let adapter = agent_event(agent, HookEvent::PreToolUse).unwrap();
            let wire = adapter
                .translate_output(&Output::PreToolUse(PreToolUseOutput::new(
                    Decision::Deny,
                    Some("Protected directory".into()),
                    Some(json!({"command": "pwd"})),
                )))
                .to_hook_output();
            assert_eq!(wire["hookSpecificOutput"]["permissionDecision"], "deny");
            assert!(wire["hookSpecificOutput"].get("updatedInput").is_none());
            let Output::PreToolUse(roundtrip) = adapter
                .parse_output(&adapter.serialize_output(&wire))
                .unwrap()
                .to_symposium()
            else {
                panic!("wrong event type")
            };
            assert_eq!(roundtrip.decision, Decision::Deny);
            assert!(roundtrip.updated_input.is_none());
        }
    }
}
