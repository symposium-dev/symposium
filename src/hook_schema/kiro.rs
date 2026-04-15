use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct Kiro;
impl Agent for Kiro {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        Some(match event {
            super::HookEvent::PreToolUse => erase_agent_hook_event(KiroPreToolUseEvent),
            super::HookEvent::PostToolUse => erase_agent_hook_event(KiroPostToolUseEvent),
            super::HookEvent::UserPromptSubmit => erase_agent_hook_event(KiroUserPromptSubmitEvent),
            super::HookEvent::SessionStart => erase_agent_hook_event(KiroSessionStartEvent),
        })
    }
}

macro_rules! kiro_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
            fn serialize_output(&self, output: &serde_json::Value) -> Vec<u8> {
                // Kiro emits plain text (stdout captured as context), not JSON.
                output
                    .get("additionalContext")
                    .and_then(|v| v.as_str())
                    .map(|s| s.as_bytes().to_vec())
                    .unwrap_or_default()
            }
        }
    };
}

kiro_event!(
    KiroPreToolUseEvent,
    KiroPreToolUseInput,
    KiroPreToolUseOutput
);
kiro_event!(
    KiroPostToolUseEvent,
    KiroPostToolUseInput,
    KiroPostToolUseOutput
);
kiro_event!(
    KiroUserPromptSubmitEvent,
    KiroUserPromptSubmitInput,
    KiroUserPromptSubmitOutput
);
kiro_event!(
    KiroSessionStartEvent,
    KiroSessionStartInput,
    KiroSessionStartOutput
);

// Kiro output: plain text stdout → additionalContext
macro_rules! kiro_output_impl {
    ($ty:ident, $variant:ident, $struct:ident { $($extra:tt)* }) => {
        impl AgentHookOutput for $ty {
            fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
                if output.is_empty() { return Ok(Self::default()); }
                let text = String::from_utf8_lossy(output);
                Ok(Self { additional_context: Some(text.into_owned()), rest: serde_json::Map::new() })
            }
            fn from_symposium(event: &symposium::OutputEvent) -> Self {
                Self { additional_context: event.additional_context().map(String::from), rest: serde_json::Map::new() }
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
pub struct KiroPreToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KiroPreToolUseOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for KiroPreToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.tool_input.clone(),
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PreToolUse(p) = event else {
            panic!("wrong event type")
        };
        Self {
            hook_event_name: "preToolUse".into(),
            tool_name: p.tool_name.clone(),
            tool_input: p.tool_input.clone(),
            cwd: p.cwd.clone(),
            session_id: p.session_id.clone(),
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

kiro_output_impl!(
    KiroPreToolUseOutput,
    PreToolUse,
    PreToolUseOutput {
        updated_input: None
    }
);

// ── PostToolUse ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroPostToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KiroPostToolUseOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for KiroPostToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PostToolUse(symposium::PostToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.tool_input.clone(),
            tool_response: self.tool_response.clone(),
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PostToolUse(p) = event else {
            panic!("wrong event type")
        };
        Self {
            hook_event_name: "postToolUse".into(),
            tool_name: p.tool_name.clone(),
            tool_input: p.tool_input.clone(),
            tool_response: p.tool_response.clone(),
            cwd: p.cwd.clone(),
            session_id: p.session_id.clone(),
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

kiro_output_impl!(KiroPostToolUseOutput, PostToolUse, PostToolUseOutput {});

// ── UserPromptSubmit ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroUserPromptSubmitInput {
    pub hook_event_name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KiroUserPromptSubmitOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for KiroUserPromptSubmitInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput {
            prompt: self.prompt.clone(),
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::UserPromptSubmit(p) = event else {
            panic!("wrong event type")
        };
        Self {
            hook_event_name: "userPromptSubmit".into(),
            prompt: p.prompt.clone(),
            cwd: p.cwd.clone(),
            session_id: p.session_id.clone(),
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

kiro_output_impl!(
    KiroUserPromptSubmitOutput,
    UserPromptSubmit,
    UserPromptSubmitOutput {}
);

// ── SessionStart (agentSpawn) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroSessionStartInput {
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KiroSessionStartOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for KiroSessionStartInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::SessionStart(symposium::SessionStartInput {
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::SessionStart(p) = event else {
            panic!("wrong event type")
        };
        Self {
            hook_event_name: "agentSpawn".into(),
            cwd: p.cwd.clone(),
            session_id: p.session_id.clone(),
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

kiro_output_impl!(KiroSessionStartOutput, SessionStart, SessionStartOutput {});
