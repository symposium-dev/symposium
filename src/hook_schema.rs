//! Hook payload types for communication between editor plugins and Symposium.

use serde::{Deserialize, Serialize};

use anyhow::{Result, anyhow};
use std::{any::Any, fmt::Debug};

use crate::config::Symposium;

pub mod claude;
pub mod copilot;
pub mod gemini;

/// Agents supported by Symposium hooks.
#[derive(Debug, Copy, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookAgent {
    #[value(name = "claude")]
    #[serde(rename = "claude")]
    Claude,
    #[value(name = "copilot")]
    #[serde(rename = "copilot")]
    Copilot,
    #[value(name = "gemini")]
    #[serde(rename = "gemini")]
    Gemini,
}

impl HookAgent {
    pub fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        match self {
            HookAgent::Claude => claude::ClaudeCode.event(event),
            HookAgent::Copilot => copilot::Copilot.event(event),
            HookAgent::Gemini => gemini::Gemini.event(event),
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

/// Top-level hook payload, as received on stdin.
///
/// Contains the typed sub-payload plus any extra fields forwarded by
/// the editor plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    #[serde(flatten)]
    pub sub_payload: HookSubPayload,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// Typed sub-payload discriminated by `hook_event_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum HookSubPayload {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUsePayload),

    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUsePayload),

    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitPayload),

    #[serde(rename = "SessionStart")]
    SessionStart(SessionStartPayload),
}

impl HookPayload {
    /// Extract the working directory from the payload, if available.
    ///
    /// Checks the typed sub-payload fields first, then falls back to
    /// the `rest` map (where agents often include `cwd` as a top-level field).
    pub fn cwd(&self) -> Option<&str> {
        match &self.sub_payload {
            HookSubPayload::PostToolUse(p) => p.cwd.as_deref(),
            HookSubPayload::UserPromptSubmit(p) => p.cwd.as_deref(),
            HookSubPayload::SessionStart(p) => p.cwd.as_deref(),
            HookSubPayload::PreToolUse(_) => None,
        }
        .or_else(|| self.rest.get("cwd").and_then(|v| v.as_str()))
    }
}

impl HookSubPayload {
    pub fn hook_event(&self) -> HookEvent {
        match self {
            HookSubPayload::PreToolUse(_) => HookEvent::PreToolUse,
            HookSubPayload::PostToolUse(_) => HookEvent::PostToolUse,
            HookSubPayload::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
            HookSubPayload::SessionStart(_) => HookEvent::SessionStart,
        }
    }

    pub fn matches_matcher(&self, matcher: &str) -> bool {
        if matcher == "*" {
            return true;
        }
        match self {
            HookSubPayload::PreToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::PostToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::UserPromptSubmit(_) => true,
            HookSubPayload::SessionStart(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUsePayload {
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUsePayload {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitPayload {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartPayload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl From<HookSubPayload> for HookPayload {
    fn from(sub_payload: HookSubPayload) -> Self {
        Self {
            sub_payload,
            rest: serde_json::Map::new(),
        }
    }
}

impl From<PreToolUsePayload> for HookPayload {
    fn from(payload: PreToolUsePayload) -> Self {
        HookSubPayload::PreToolUse(payload).into()
    }
}

impl From<PostToolUsePayload> for HookPayload {
    fn from(payload: PostToolUsePayload) -> Self {
        HookSubPayload::PostToolUse(payload).into()
    }
}

impl From<UserPromptSubmitPayload> for HookPayload {
    fn from(payload: UserPromptSubmitPayload) -> Self {
        HookSubPayload::UserPromptSubmit(payload).into()
    }
}

impl From<SessionStartPayload> for HookPayload {
    fn from(payload: SessionStartPayload) -> Self {
        HookSubPayload::SessionStart(payload).into()
    }
}

/// Structured output from builtin hook handlers.
///
/// Serialized to JSON on stdout for Claude Code to consume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookOutput {
    /// If set, injected into the LLM conversation as additional context.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl HookOutput {
    /// Create a HookOutput with additional context for the given event.
    pub fn with_context(event_name: &str, context: String) -> Self {
        Self {
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: event_name.to_string(),
                additional_context: Some(context),
                updated_input: None,
                rest: serde_json::Map::new(),
            }),
            rest: serde_json::Map::new(),
        }
    }

    /// Create an empty HookOutput (no additional context).
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Represents the data sent *from* an agent *to* a hook.
pub trait AgentHookPayload: Debug {
    /// Parse an incoming JSON payload string into a concrete payload struct.
    fn parse_payload(payload: &str) -> Result<Self>
    where
        Self: Sized;
    /// Convert this payload into the generic `HookPayload` for builtin hook handlers.
    fn to_hook_payload(&self) -> HookPayload;
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
    /// Convert a generic `HookOutput` from builtin hook handlers into this output struct.
    fn from_hook_output(output: &HookOutput) -> anyhow::Result<Self>
    where
        Self: Sized;
    /// Convert this output into a JSON value to return to the agent.
    fn to_hook_output(&self) -> serde_json::Value;

    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

/// Represents some hook event-handler for a specific agent
pub trait AgentHookEvent {
    type Payload: AgentHookPayload;
    type Output: AgentHookOutput;

    /// Parse an incoming JSON payload string into a concrete payload struct.
    fn parse_payload(&self, payload: &str) -> anyhow::Result<Self::Payload> {
        Self::Payload::parse_payload(payload)
    }
    /// Parse raw stdout bytes from a hook handler into a concrete output struct.
    fn parse_output(&self, output: &[u8]) -> anyhow::Result<Self::Output> {
        Self::Output::parse_output(output)
    }
    /// Convert a generic `HookOutput` from builtin hook handlers into this output struct.
    fn from_hook_output(&self, output: &HookOutput) -> anyhow::Result<Self::Output> {
        Self::Output::from_hook_output(output)
    }
    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output;
    fn dispatch_plugin_hooks(
        &self,
        sym: &Symposium,
        payload: &Self::Payload,
        prior_output: Self::Output,
    ) -> crate::hook::PluginHookOutput
    where
        Self: Sized,
    {
        crate::hook::dispatch_plugin_hooks::<Self>(sym, self, payload, prior_output)
    }
}

/// Represents an agent that can handle hook events.
pub trait Agent {
    fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>>;
}

pub trait ErasedAgentHookEvent {
    /// Parse an incoming JSON payload into a boxed `AgentHookPayload`.
    fn parse_payload(&self, payload: &str) -> Result<Box<dyn AgentHookPayload>>;

    /// Parses a stdout into a boxed `AgentHookOutput`.
    fn parse_output(&self, output: &[u8]) -> Result<Box<dyn AgentHookOutput>>;

    /// Parse a builtin `HookOutput` into a boxed `AgentHookOutput`.
    fn from_hook_output(&self, output: &HookOutput) -> Result<Box<dyn AgentHookOutput>>;

    fn dispatch_plugin_hooks(
        &self,
        _sym: &Symposium,
        payload: Box<dyn AgentHookPayload>,
        prior_output: Box<dyn AgentHookOutput>,
    ) -> crate::hook::PluginHookOutput;
}

struct ErasedAgentHookEventImpl<E: AgentHookEvent + 'static>(E);

impl<E> ErasedAgentHookEvent for ErasedAgentHookEventImpl<E>
where
    E: AgentHookEvent + 'static,
    E::Payload: AgentHookPayload + 'static,
    E::Output: AgentHookOutput + 'static,
{
    fn parse_payload(&self, payload: &str) -> Result<Box<dyn AgentHookPayload>> {
        let p = self.0.parse_payload(payload)?;
        Ok(Box::new(p))
    }

    fn parse_output(&self, output: &[u8]) -> Result<Box<dyn AgentHookOutput>> {
        let o = self.0.parse_output(output)?;
        Ok(Box::new(o))
    }
    fn from_hook_output(&self, output: &HookOutput) -> Result<Box<dyn AgentHookOutput>> {
        let o = self.0.from_hook_output(output)?;
        Ok(Box::new(o))
    }

    fn dispatch_plugin_hooks(
        &self,
        _sym: &Symposium,
        payload: Box<dyn AgentHookPayload>,
        prior_output: Box<dyn AgentHookOutput>,
    ) -> crate::hook::PluginHookOutput {
        let payload_any = payload.into_any();
        let payload_concrete = payload_any
            .downcast::<E::Payload>()
            .map_err(|_| anyhow!("failed to downcast payload"))
            .unwrap();
        let output_any = prior_output.into_any();
        let output_concrete = output_any
            .downcast::<E::Output>()
            .map_err(|_| anyhow!("failed to downcast output"))
            .unwrap();
        let output = self
            .0
            .dispatch_plugin_hooks(_sym, &payload_concrete, *output_concrete);
        output
    }
}

/// Helper to erase a concrete `AgentHookEvent` into a trait object.
pub fn erase_agent_hook_event<E>(e: E) -> Box<dyn ErasedAgentHookEvent>
where
    E: AgentHookEvent + 'static,
    E::Payload: AgentHookPayload + 'static,
    E::Output: AgentHookOutput + 'static,
{
    Box::new(ErasedAgentHookEventImpl(e))
}
