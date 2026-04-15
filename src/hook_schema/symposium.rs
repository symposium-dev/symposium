//! Canonical symposium hook types — the "lingua franca" between agents.
//!
//! Each agent module converts to/from these types. Builtin dispatch
//! operates entirely on these types.

use serde::{Deserialize, Serialize};

use super::HookEvent;

// ── Input types ───────────────────────────────────────────────────────

/// Canonical PreToolUse input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Canonical PostToolUse input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Canonical UserPromptSubmit input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitInput {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Canonical SessionStart input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartInput {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Enum over all canonical input types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    UserPromptSubmit(UserPromptSubmitInput),
    SessionStart(SessionStartInput),
}

impl InputEvent {
    pub fn event(&self) -> HookEvent {
        match self {
            InputEvent::PreToolUse(_) => HookEvent::PreToolUse,
            InputEvent::PostToolUse(_) => HookEvent::PostToolUse,
            InputEvent::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
            InputEvent::SessionStart(_) => HookEvent::SessionStart,
        }
    }

    pub fn cwd(&self) -> Option<&str> {
        match self {
            InputEvent::PreToolUse(p) => p.cwd.as_deref(),
            InputEvent::PostToolUse(p) => p.cwd.as_deref(),
            InputEvent::UserPromptSubmit(p) => p.cwd.as_deref(),
            InputEvent::SessionStart(p) => p.cwd.as_deref(),
        }
    }

    pub fn matches_matcher(&self, matcher: &str) -> bool {
        if matcher == "*" {
            return true;
        }
        match self {
            InputEvent::PreToolUse(p) => matcher.contains(&p.tool_name),
            InputEvent::PostToolUse(p) => matcher.contains(&p.tool_name),
            InputEvent::UserPromptSubmit(_) => true,
            InputEvent::SessionStart(_) => true,
        }
    }
}

// ── Output types ──────────────────────────────────────────────────────

/// Canonical PreToolUse output — can inject context or modify input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreToolUseOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
    #[serde(
        rename = "updatedInput",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_input: Option<serde_json::Value>,
}

/// Canonical PostToolUse output — can inject context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostToolUseOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

/// Canonical UserPromptSubmit output — can inject context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserPromptSubmitOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

/// Canonical SessionStart output — can inject context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStartOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

/// Enum over all canonical output types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputEvent {
    PreToolUse(PreToolUseOutput),
    PostToolUse(PostToolUseOutput),
    UserPromptSubmit(UserPromptSubmitOutput),
    SessionStart(SessionStartOutput),
}

impl OutputEvent {
    /// Create an empty output for the given event.
    pub fn empty_for(event: HookEvent) -> Self {
        match event {
            HookEvent::PreToolUse => OutputEvent::PreToolUse(PreToolUseOutput::default()),
            HookEvent::PostToolUse => OutputEvent::PostToolUse(PostToolUseOutput::default()),
            HookEvent::UserPromptSubmit => {
                OutputEvent::UserPromptSubmit(UserPromptSubmitOutput::default())
            }
            HookEvent::SessionStart => OutputEvent::SessionStart(SessionStartOutput::default()),
        }
    }

    /// Create an output with additional context for the given event.
    pub fn with_context(event: HookEvent, context: String) -> Self {
        match event {
            HookEvent::PreToolUse => OutputEvent::PreToolUse(PreToolUseOutput {
                additional_context: Some(context),
                updated_input: None,
            }),
            HookEvent::PostToolUse => OutputEvent::PostToolUse(PostToolUseOutput {
                additional_context: Some(context),
            }),
            HookEvent::UserPromptSubmit => OutputEvent::UserPromptSubmit(UserPromptSubmitOutput {
                additional_context: Some(context),
            }),
            HookEvent::SessionStart => OutputEvent::SessionStart(SessionStartOutput {
                additional_context: Some(context),
            }),
        }
    }

    /// Extract additional_context from any variant.
    pub fn additional_context(&self) -> Option<&str> {
        match self {
            OutputEvent::PreToolUse(o) => o.additional_context.as_deref(),
            OutputEvent::PostToolUse(o) => o.additional_context.as_deref(),
            OutputEvent::UserPromptSubmit(o) => o.additional_context.as_deref(),
            OutputEvent::SessionStart(o) => o.additional_context.as_deref(),
        }
    }
}

// ── AgentHookInput for InputEvent ────────────────────────────────────
// Allows symposium-format plugins to receive canonical InputEvent JSON.

impl super::AgentHookInput for InputEvent {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> InputEvent {
        self.clone()
    }
    fn from_symposium(event: &InputEvent) -> Self {
        event.clone()
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
