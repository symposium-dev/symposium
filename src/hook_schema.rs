//! Hook payload types for communication between editor plugins and Symposium.

use serde::{Deserialize, Serialize};

// FIXME: We really need a "core" set of hook events and expected data. But then
// have "adapters" for each different agent. The interesting bit is connecting
// builtin hooks (which should handle the core set of events) to the agent-specific
// formats.
// 
/// For now, this is very much designed around Claude Code's expected hook payloads.

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

/// Structured output from builtin hook handlers.
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
