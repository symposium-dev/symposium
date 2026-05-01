use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct ClaudeCode;
impl Agent for ClaudeCode {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        Some(match event {
            super::HookEvent::PreToolUse => erase_agent_hook_event(ClaudePreToolUseEvent),
            super::HookEvent::PostToolUse => erase_agent_hook_event(ClaudePostToolUseEvent),
            super::HookEvent::UserPromptSubmit => {
                erase_agent_hook_event(ClaudeUserPromptSubmitEvent)
            }
            super::HookEvent::SessionStart => erase_agent_hook_event(ClaudeSessionStartEvent),
        })
    }
}

macro_rules! claude_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

claude_event!(
    ClaudePreToolUseEvent,
    ClaudePreToolUseInput,
    ClaudePreToolUseOutput
);
claude_event!(
    ClaudePostToolUseEvent,
    ClaudePostToolUseInput,
    ClaudePostToolUseOutput
);
claude_event!(
    ClaudeUserPromptSubmitEvent,
    ClaudeUserPromptSubmitInput,
    ClaudeUserPromptSubmitOutput
);
claude_event!(
    ClaudeSessionStartEvent,
    ClaudeSessionStartInput,
    ClaudeSessionStartOutput
);

// ── Helper: extract context from symposium output ─────────────────────

fn claude_hook_output_from_symposium(
    event_name: &str,
    event: &symposium::OutputEvent,
) -> Option<serde_json::Value> {
    let ctx = event.additional_context()?;
    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event_name,
            "additionalContext": ctx,
        }
    }))
}

// ── PreToolUse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePreToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudePreToolUseOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<ClaudePreToolUseHookOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePreToolUseHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for ClaudePreToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.rest.get("tool_input").cloned().unwrap_or_default(),
            session_id: self
                .rest
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            cwd: self
                .rest
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PreToolUse(p) = event else {
            panic!("wrong event type")
        };
        let mut rest = serde_json::Map::new();
        if let Some(s) = &p.session_id {
            rest.insert("session_id".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(c) = &p.cwd {
            rest.insert("cwd".into(), serde_json::Value::String(c.clone()));
        }
        rest.insert("tool_input".into(), p.tool_input.clone());
        Self {
            hook_event_name: "PreToolUse".into(),
            tool_name: p.tool_name.clone(),
            rest,
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for ClaudePreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        match claude_hook_output_from_symposium("PreToolUse", event) {
            Some(v) => serde_json::from_value(v).unwrap_or_default(),
            None => Self::default(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        let h = self.hook_specific_output.as_ref();
        symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput {
            additional_context: h.and_then(|h| h.additional_context.clone()),
            updated_input: h.and_then(|h| h.updated_input.clone()),
        })
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_tool_use_updated_input_preserves_json_object() {
        let hook_output = ClaudePreToolUseOutput {
            hook_specific_output: Some(ClaudePreToolUseHookOutput {
                hook_event_name: "PreToolUse".into(),
                permission_decision: None,
                permission_decision_reason: None,
                updated_input: Some(serde_json::json!({"command": "safe-cmd"})),
                additional_context: Some("context".into()),
                rest: serde_json::Map::new(),
            }),
            ..Default::default()
        };

        let symposium::OutputEvent::PreToolUse(output) = hook_output.to_symposium() else {
            panic!("wrong output type")
        };

        assert_eq!(output.additional_context.as_deref(), Some("context"));
        assert_eq!(
            output.updated_input,
            Some(serde_json::json!({"command": "safe-cmd"}))
        );
    }
}

// ── PostToolUse ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePostToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudePostToolUseOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<ClaudePostToolUseHookOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePostToolUseHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for ClaudePostToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PostToolUse(symposium::PostToolUseInput {
            tool_name: self.tool_name.clone(),
            tool_input: self.tool_input.clone(),
            tool_response: self.tool_response.clone(),
            session_id: self
                .rest
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            cwd: self
                .rest
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PostToolUse(p) = event else {
            panic!("wrong event type")
        };
        let mut rest = serde_json::Map::new();
        if let Some(s) = &p.session_id {
            rest.insert("session_id".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(c) = &p.cwd {
            rest.insert("cwd".into(), serde_json::Value::String(c.clone()));
        }
        Self {
            hook_event_name: "PostToolUse".into(),
            tool_name: p.tool_name.clone(),
            tool_input: p.tool_input.clone(),
            tool_response: p.tool_response.clone(),
            rest,
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for ClaudePostToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        match claude_hook_output_from_symposium("PostToolUse", event) {
            Some(v) => serde_json::from_value(v).unwrap_or_default(),
            None => Self::default(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::PostToolUse(symposium::PostToolUseOutput {
            additional_context: self
                .hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        })
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── UserPromptSubmit ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeUserPromptSubmitInput {
    pub hook_event_name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeUserPromptSubmitOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<ClaudeUserPromptSubmitHookOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeUserPromptSubmitHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for ClaudeUserPromptSubmitInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput {
            prompt: self.prompt.clone(),
            session_id: self
                .rest
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            cwd: self
                .rest
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::UserPromptSubmit(p) = event else {
            panic!("wrong event type")
        };
        let mut rest = serde_json::Map::new();
        if let Some(s) = &p.session_id {
            rest.insert("session_id".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(c) = &p.cwd {
            rest.insert("cwd".into(), serde_json::Value::String(c.clone()));
        }
        Self {
            hook_event_name: "UserPromptSubmit".into(),
            prompt: p.prompt.clone(),
            rest,
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for ClaudeUserPromptSubmitOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        match claude_hook_output_from_symposium("UserPromptSubmit", event) {
            Some(v) => serde_json::from_value(v).unwrap_or_default(),
            None => Self::default(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::UserPromptSubmit(symposium::UserPromptSubmitOutput {
            additional_context: self
                .hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        })
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── SessionStart ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSessionStartInput {
    pub hook_event_name: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeSessionStartOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<ClaudeSessionStartHookOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSessionStartHookOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for ClaudeSessionStartInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::SessionStart(symposium::SessionStartInput {
            session_id: self
                .rest
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            cwd: self
                .rest
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::SessionStart(p) = event else {
            panic!("wrong event type")
        };
        let mut rest = serde_json::Map::new();
        if let Some(s) = &p.session_id {
            rest.insert("session_id".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(c) = &p.cwd {
            rest.insert("cwd".into(), serde_json::Value::String(c.clone()));
        }
        Self {
            hook_event_name: "SessionStart".into(),
            rest,
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for ClaudeSessionStartOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        match claude_hook_output_from_symposium("SessionStart", event) {
            Some(v) => serde_json::from_value(v).unwrap_or_default(),
            None => Self::default(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::SessionStart(symposium::SessionStartOutput {
            additional_context: self
                .hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        })
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
