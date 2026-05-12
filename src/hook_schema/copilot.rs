use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

/// Copilot sends `toolArgs` as a JSON-encoded string (per Copilot's hook protocol),
/// while the canonical `PreToolUseInput.tool_input` is a structured `Value`. Parse
/// the string into a `Value` when crossing into canonical form. If parsing fails
/// or the field is already structured, pass it through unchanged.
fn parse_copilot_tool_args(raw: &serde_json::Value) -> serde_json::Value {
    if let Some(s) = raw.as_str()
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
    {
        return v;
    }
    raw.clone()
}

/// Inverse of [`parse_copilot_tool_args`]: when writing out to Copilot wire format,
/// re-encode a structured `Value` back into a JSON string.
fn stringify_for_copilot(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::String(_) => v.clone(),
        _ => serde_json::Value::String(v.to_string()),
    }
}

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
            tool_input: parse_copilot_tool_args(&self.tool_args),
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
            tool_args: stringify_for_copilot(&p.tool_input),
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
            tool_input: parse_copilot_tool_args(&self.tool_args),
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
            tool_args: stringify_for_copilot(&p.tool_input),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_tool_use_parses_string_tool_args() {
        let raw = r#"{"toolName":"bash","toolArgs":"{\"command\":\"ls\"}"}"#;
        let input: CopilotPreToolUseInput = serde_json::from_str(raw).unwrap();
        let symposium::InputEvent::PreToolUse(canon) = input.to_symposium() else {
            panic!("wrong event type")
        };
        assert_eq!(canon.tool_input, serde_json::json!({"command": "ls"}));
    }

    #[test]
    fn pre_tool_use_round_trips_object_to_copilot_string() {
        let canon = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput {
            tool_name: "bash".into(),
            tool_input: serde_json::json!({"command": "ls"}),
            session_id: None,
            cwd: None,
        });
        let out = CopilotPreToolUseInput::from_symposium(&canon);
        let s = out
            .tool_args
            .as_str()
            .expect("toolArgs must be a JSON string on the Copilot wire");
        let reparsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(reparsed, serde_json::json!({"command": "ls"}));
    }

    #[test]
    fn post_tool_use_parses_string_tool_args() {
        let raw = r#"{"toolName":"bash","toolArgs":"{\"command\":\"ls\"}","toolResponse":{}}"#;
        let input: CopilotPostToolUseInput = serde_json::from_str(raw).unwrap();
        let symposium::InputEvent::PostToolUse(canon) = input.to_symposium() else {
            panic!("wrong event type")
        };
        assert_eq!(canon.tool_input, serde_json::json!({"command": "ls"}));
    }

    #[test]
    fn post_tool_use_round_trips_object_to_copilot_string() {
        let canon = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput {
            tool_name: "bash".into(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: None,
            cwd: None,
        });
        let out = CopilotPostToolUseInput::from_symposium(&canon);
        let s = out
            .tool_args
            .as_str()
            .expect("toolArgs must be a JSON string on the Copilot wire");
        let reparsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(reparsed, serde_json::json!({"command": "ls"}));
    }
}
