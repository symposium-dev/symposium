use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct Copilot;
impl Agent for Copilot {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        Some(match event {
            super::HookEvent::PreToolUse => erase_agent_hook_event(CopilotPreToolUseEvent),
            super::HookEvent::PostToolUse => erase_agent_hook_event(CopilotPostToolUseEvent),
            super::HookEvent::UserPromptSubmit => {
                erase_agent_hook_event(CopilotUserPromptSubmitEvent)
            }
            super::HookEvent::SessionStart => erase_agent_hook_event(CopilotSessionStartEvent),
        })
    }
}

macro_rules! copilot_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

copilot_event!(
    CopilotPreToolUseEvent,
    CopilotPreToolUseInput,
    CopilotPreToolUseOutput
);
copilot_event!(
    CopilotPostToolUseEvent,
    CopilotPostToolUseInput,
    CopilotPostToolUseOutput
);
copilot_event!(
    CopilotUserPromptSubmitEvent,
    CopilotUserPromptSubmitInput,
    CopilotUserPromptSubmitOutput
);
copilot_event!(
    CopilotSessionStartEvent,
    CopilotSessionStartInput,
    CopilotSessionStartOutput
);

// Copilot output is flat (additionalContext at top level, no hookSpecificOutput).

macro_rules! copilot_output_impl {
    ($ty:ident, $variant:ident, $struct:ident { $($extra:tt)* }) => {
        impl AgentHookOutput for $ty {
            fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
                if output.is_empty() { return Ok(Self::default()); }
                Ok(serde_json::from_slice(output)?)
            }
            fn from_symposium(event: &symposium::OutputEvent) -> Self {
                let mut out = Self::default();
                out.additional_context = event.additional_context().map(String::from);
                out
            }
            fn to_symposium(&self) -> symposium::OutputEvent {
                symposium::OutputEvent::$variant(symposium::$struct {
                    additional_context: self.additional_context.clone(),
                    $($extra)*
                })
            }
            fn to_hook_output(&self) -> serde_json::Value { serde_json::to_value(self).unwrap() }
            fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> { self }
        }
    };
}

// ── PreToolUse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotPreToolUseInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "toolArgs", default)]
    pub tool_args: serde_json::Value,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CopilotPreToolUseOutput {
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(rename = "modifiedArgs", skip_serializing_if = "Option::is_none")]
    pub modified_args: Option<serde_json::Value>,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CopilotPreToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.tool_args.clone(),
            session_id: None,
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PreToolUse(p) = event else {
            panic!("wrong event type")
        };
        Self {
            timestamp: None,
            cwd: p.cwd.clone(),
            tool_name: p.tool_name.clone(),
            tool_args: p.tool_input.clone(),
            rest: serde_json::Map::new(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

copilot_output_impl!(
    CopilotPreToolUseOutput,
    PreToolUse,
    PreToolUseOutput {
        updated_input: None
    }
);

// ── PostToolUse ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotPostToolUseInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "toolArgs", default)]
    pub tool_args: serde_json::Value,
    #[serde(rename = "toolResponse", default)]
    pub tool_response: serde_json::Value,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CopilotPostToolUseOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CopilotPostToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PostToolUse(symposium::PostToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.tool_args.clone(),
            tool_response: self.tool_response.clone(),
            session_id: None,
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PostToolUse(p) = event else {
            panic!("wrong event type")
        };
        Self {
            timestamp: None,
            cwd: p.cwd.clone(),
            tool_name: p.tool_name.clone(),
            tool_args: p.tool_input.clone(),
            tool_response: p.tool_response.clone(),
            rest: serde_json::Map::new(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

copilot_output_impl!(CopilotPostToolUseOutput, PostToolUse, PostToolUseOutput {});

// ── UserPromptSubmit ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotUserPromptSubmitInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CopilotUserPromptSubmitOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CopilotUserPromptSubmitInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput {
            prompt: self.prompt.clone(),
            session_id: None,
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::UserPromptSubmit(p) = event else {
            panic!("wrong event type")
        };
        Self {
            timestamp: None,
            cwd: p.cwd.clone(),
            prompt: p.prompt.clone(),
            rest: serde_json::Map::new(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

copilot_output_impl!(
    CopilotUserPromptSubmitOutput,
    UserPromptSubmit,
    UserPromptSubmitOutput {}
);

// ── SessionStart ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotSessionStartInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CopilotSessionStartOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CopilotSessionStartInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::SessionStart(symposium::SessionStartInput {
            session_id: None,
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::SessionStart(p) = event else {
            panic!("wrong event type")
        };
        Self {
            timestamp: None,
            cwd: p.cwd.clone(),
            rest: serde_json::Map::new(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

copilot_output_impl!(
    CopilotSessionStartOutput,
    SessionStart,
    SessionStartOutput {}
);
