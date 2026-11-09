use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct Codex;
impl Agent for Codex {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(CodexPreToolUseEvent)),
            super::HookEvent::PostToolUse => Some(erase_agent_hook_event(CodexPostToolUseEvent)),
            super::HookEvent::UserPromptSubmit => {
                Some(erase_agent_hook_event(CodexUserPromptSubmitEvent))
            }
            super::HookEvent::SessionStart => Some(erase_agent_hook_event(CodexSessionStartEvent)),
            _ => None,
        }
    }
}

macro_rules! codex_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

codex_event!(
    CodexPreToolUseEvent,
    CodexPreToolUseInput,
    CodexPreToolUseOutput
);
codex_event!(
    CodexPostToolUseEvent,
    CodexPostToolUseInput,
    CodexPostToolUseOutput
);
codex_event!(
    CodexUserPromptSubmitEvent,
    CodexUserPromptSubmitInput,
    CodexUserPromptSubmitOutput
);
codex_event!(
    CodexSessionStartEvent,
    CodexSessionStartInput,
    CodexSessionStartOutput
);

fn codex_hook_output_from_symposium(
    event_name: &str,
    event: &symposium::OutputEvent,
) -> Option<serde_json::Value> {
    let ctx = event.additional_context()?;
    Some(
        serde_json::json!({ "hookSpecificOutput": { "hookEventName": event_name, "additionalContext": ctx } }),
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexHookSpecificOutput {
    #[serde(rename = "hookEventName", default)]
    pub hook_event_name: String,
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

// ── PreToolUse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPreToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexPreToolUseOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CodexHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CodexPreToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            self.tool_name.clone(),
            self.tool_input.clone(),
            self.session_id.clone(),
            self.cwd.clone(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PreToolUse(p) = event else {
            panic!("wrong event")
        };
        Self {
            hook_event_name: "PreToolUse".into(),
            tool_name: p.tool_name.clone(),
            tool_input: p.tool_input.clone(),
            tool_use_id: None,
            session_id: p.session_id.clone(),
            cwd: p.cwd.clone(),
            model: None,
            turn_id: None,
            rest: Default::default(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for CodexPreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        let output = super::claude::ClaudePreToolUseOutput::from_symposium(event);
        Self {
            hook_specific_output: output
                .hook_specific_output
                .map(|h| CodexHookSpecificOutput {
                    hook_event_name: h.hook_event_name,
                    permission_decision: h.permission_decision,
                    permission_decision_reason: h.permission_decision_reason,
                    additional_context: h.additional_context,
                    updated_input: h.updated_input,
                    rest: h.rest,
                }),
            ..Default::default()
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        let h = self.hook_specific_output.as_ref();
        let decision = match h
            .and_then(|h| h.permission_decision.as_deref())
            .or(self.decision.as_deref())
        {
            Some("block") | Some("deny") => crate::hook::Decision::Deny,
            _ => crate::hook::Decision::Allow,
        };
        symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::new(
            decision,
            h.and_then(|h| h.additional_context.clone())
                .or_else(|| h.and_then(|h| h.permission_decision_reason.clone()))
                .or_else(|| self.reason.clone()),
            h.and_then(|h| h.updated_input.clone()),
        ))
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
    use crate::hook::Decision;
    use serde_json::json;

    #[test]
    fn modern_pre_tool_fields_override_legacy_decision() {
        for (modern, legacy, expected) in [
            ("allow", "block", Decision::Allow),
            ("deny", "allow", Decision::Deny),
        ] {
            let wire = serde_json::to_vec(&json!({
                "decision": legacy,
                "reason": "Legacy reason",
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": modern,
                    "permissionDecisionReason": "Modern reason",
                    "updatedInput": {"command": "pwd"}
                }
            }))
            .unwrap();
            let symposium::OutputEvent::PreToolUse(output) =
                CodexPreToolUseOutput::parse_output(&wire)
                    .unwrap()
                    .to_symposium()
            else {
                panic!("wrong event type")
            };
            assert_eq!(output.decision, expected);
            assert_eq!(output.additional_context.as_deref(), Some("Modern reason"));
            assert_eq!(output.updated_input, Some(json!({"command": "pwd"})));
        }
    }

    #[test]
    fn legacy_denial_keeps_its_reason() {
        let symposium::OutputEvent::PreToolUse(output) = CodexPreToolUseOutput::parse_output(
            br#"{"decision":"block","reason":"Protected directory"}"#,
        )
        .unwrap()
        .to_symposium() else {
            panic!("wrong event type")
        };
        assert_eq!(output.decision, Decision::Deny);
        assert_eq!(
            output.additional_context.as_deref(),
            Some("Protected directory")
        );
    }
}

// ── PostToolUse ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPostToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexPostToolUseOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CodexHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CodexPostToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            self.tool_name.clone(),
            self.tool_input.clone(),
            self.tool_response.clone(),
            self.session_id.clone(),
            self.cwd.clone(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PostToolUse(p) = event else {
            panic!("wrong event")
        };
        Self {
            hook_event_name: "PostToolUse".into(),
            tool_name: p.tool_name.clone(),
            tool_input: p.tool_input.clone(),
            tool_response: p.tool_response.clone(),
            tool_use_id: None,
            session_id: p.session_id.clone(),
            cwd: p.cwd.clone(),
            model: None,
            turn_id: None,
            rest: Default::default(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for CodexPostToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        codex_hook_output_from_symposium("PostToolUse", event)
            .map(|v| serde_json::from_value(v).unwrap())
            .unwrap_or_default()
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::PostToolUse(symposium::PostToolUseOutput::new(
            self.hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        ))
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
pub struct CodexUserPromptSubmitInput {
    pub hook_event_name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexUserPromptSubmitOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CodexHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CodexUserPromptSubmitInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput::new(
            self.prompt.clone(),
            self.session_id.clone(),
            self.cwd.clone(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::UserPromptSubmit(p) = event else {
            panic!("wrong event")
        };
        Self {
            hook_event_name: "UserPromptSubmit".into(),
            prompt: p.prompt.clone(),
            session_id: p.session_id.clone(),
            cwd: p.cwd.clone(),
            model: None,
            turn_id: None,
            rest: Default::default(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for CodexUserPromptSubmitOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        codex_hook_output_from_symposium("UserPromptSubmit", event)
            .map(|v| serde_json::from_value(v).unwrap())
            .unwrap_or_default()
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::UserPromptSubmit(symposium::UserPromptSubmitOutput::new(
            self.hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        ))
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
pub struct CodexSessionStartInput {
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexSessionStartOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CodexHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookInput for CodexSessionStartInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::SessionStart(symposium::SessionStartInput::new(
            self.session_id.clone(),
            self.cwd.clone(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::SessionStart(p) = event else {
            panic!("wrong event")
        };
        Self {
            hook_event_name: "SessionStart".into(),
            session_id: p.session_id.clone(),
            cwd: p.cwd.clone(),
            model: None,
            rest: Default::default(),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for CodexSessionStartOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        codex_hook_output_from_symposium("SessionStart", event)
            .map(|v| serde_json::from_value(v).unwrap())
            .unwrap_or_default()
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::SessionStart(symposium::SessionStartOutput::new(
            self.hook_specific_output
                .as_ref()
                .and_then(|h| h.additional_context.clone()),
        ))
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
