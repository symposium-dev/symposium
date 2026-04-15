//! Hook payload types for communication between editor plugins and Symposium.

use serde::{Deserialize, Serialize};

use anyhow::Result;
use std::{any::Any, fmt::Debug};

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod gemini;
pub mod goose;
pub mod kiro;
pub mod opencode;
pub mod symposium;

/// Agents supported by Symposium hooks.
#[derive(Debug, Copy, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookAgent {
    #[value(name = "claude")]
    #[serde(rename = "claude")]
    Claude,
    #[value(name = "codex")]
    #[serde(rename = "codex")]
    Codex,
    #[value(name = "copilot")]
    #[serde(rename = "copilot")]
    Copilot,
    #[value(name = "gemini")]
    #[serde(rename = "gemini")]
    Gemini,
    #[value(name = "goose")]
    #[serde(rename = "goose")]
    Goose,
    #[value(name = "kiro")]
    #[serde(rename = "kiro")]
    Kiro,
    #[value(name = "opencode")]
    #[serde(rename = "opencode")]
    OpenCode,
}

impl HookAgent {
    pub fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        match self {
            HookAgent::Claude => claude::ClaudeCode.event(event),
            HookAgent::Codex => codex::Codex.event(event),
            HookAgent::Copilot => copilot::Copilot.event(event),
            HookAgent::Gemini => gemini::Gemini.event(event),
            HookAgent::Goose => goose::Goose.event(event),
            HookAgent::Kiro => kiro::Kiro.event(event),
            HookAgent::OpenCode => opencode::OpenCode.event(event),
        }
    }
}

/// Hook event types supported by Symposium.
#[derive(Debug, Copy, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookEvent {
    #[value(name = "pre-tool-use")]
    #[serde(rename = "PreToolUse")]
    PreToolUse,

    #[value(name = "post-tool-use")]
    #[serde(rename = "PostToolUse")]
    PostToolUse,

    #[value(name = "user-prompt-submit")]
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,

    #[value(name = "session-start")]
    #[serde(rename = "SessionStart")]
    SessionStart,
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
    fn from_symposium_output(&self, output: &symposium::OutputEvent) -> Self::Output {
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
    fn from_symposium_output(&self, output: &symposium::OutputEvent) -> Box<dyn AgentHookOutput>;

    /// Convert a canonical symposium input event into a boxed agent payload.
    fn from_symposium_input(&self, input: &symposium::InputEvent) -> Box<dyn AgentHookInput>;

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
    fn from_symposium_output(&self, output: &symposium::OutputEvent) -> Box<dyn AgentHookOutput> {
        Box::new(self.0.from_symposium_output(output))
    }

    fn from_symposium_input(&self, input: &symposium::InputEvent) -> Box<dyn AgentHookInput> {
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
