//! Hook payload types for Symposium.
//!
//! This crate provides the schema types used for communication between
//! Claude Code (or other editor plugins) and Symposium hook handlers.
//! Extracting them into a separate crate allows other tools to produce
//! or consume hook payloads without depending on the full Symposium binary.

use serde::{Deserialize, Serialize};

/// Hook event types supported by Symposium.
#[derive(Debug, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
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
}

impl HookSubPayload {
    pub fn hook_event(&self) -> HookEvent {
        match self {
            HookSubPayload::PreToolUse(_) => HookEvent::PreToolUse,
            HookSubPayload::PostToolUse(_) => HookEvent::PostToolUse,
            HookSubPayload::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
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

/// Structured output from hook handlers.
///
/// Serialized to JSON on stdout for Claude Code to consume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookOutput {
    /// If set, injected into the LLM conversation as additional context.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

impl HookOutput {
    /// Create a HookOutput with additional context for the given event.
    pub fn with_context(event_name: &str, context: String) -> Self {
        Self {
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: event_name.to_string(),
                additional_context: Some(context),
            }),
        }
    }

    /// Create an empty HookOutput (no additional context).
    pub fn empty() -> Self {
        Self::default()
    }
}
