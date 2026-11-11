//! Command Code hook wire format.
//!
//! Command Code reads Claude-shaped hooks from `.commandcode/settings.json`
//! and speaks JSON on stdin/stdout. It differs from Claude in three ways that
//! matter here:
//!
//! - It has no `UserPromptSubmit`, and Symposium does not register `Stop`, so
//!   the events handled are `PreToolUse`, `PostToolUse` and `SessionStart`.
//! - Its tools have their own names and argument fields (`shell_command`,
//!   `read_file`, `write_file`, `edit_file`). As for every other agent, they
//!   reach plugin hooks exactly as Command Code sends them.
//! - `PreToolUse` can allow or deny and attach context, but has no
//!   `updatedInput` and no `ask`, so a plugin's rewrite cannot be applied.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct CommandCode;
impl Agent for CommandCode {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => {
                Some(erase_agent_hook_event(CommandCodePreToolUseEvent))
            }
            super::HookEvent::PostToolUse => {
                Some(erase_agent_hook_event(CommandCodePostToolUseEvent))
            }
            super::HookEvent::SessionStart => {
                Some(erase_agent_hook_event(CommandCodeSessionStartEvent))
            }
            _ => None,
        }
    }
}

macro_rules! command_code_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

command_code_event!(
    CommandCodePreToolUseEvent,
    CommandCodePreToolUseInput,
    CommandCodePreToolUseOutput
);
command_code_event!(
    CommandCodePostToolUseEvent,
    CommandCodePostToolUseInput,
    CommandCodePostToolUseOutput
);
command_code_event!(
    CommandCodeSessionStartEvent,
    CommandCodeSessionStartInput,
    CommandCodeSessionStartOutput
);

fn command_code_hook_output_from_symposium(
    event_name: &str,
    event: &symposium::OutputEvent,
) -> Option<Value> {
    let ctx = event.additional_context()?;
    Some(
        serde_json::json!({ "hookSpecificOutput": { "hookEventName": event_name, "additionalContext": ctx } }),
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandCodeHookSpecificOutput {
    #[serde(rename = "hookEventName", default)]
    pub hook_event_name: String,
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
    #[serde(
        rename = "permissionDecision",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision: Option<String>,
    #[serde(
        rename = "permissionDecisionReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

// ── PreToolUse ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCodePreToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandCodePreToolUseOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CommandCodeHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl AgentHookInput for CommandCodePreToolUseInput {
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

impl AgentHookOutput for CommandCodePreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        let symposium::OutputEvent::PreToolUse(o) = event else {
            return Self::default();
        };
        if o.updated_input.is_some() {
            // `PreToolUse` output has no `updatedInput` and no `ask`, so a
            // rewrite cannot be applied here, and applying nothing is safer
            // than approving the original call as if it had been rewritten.
            tracing::warn!(
                "Command Code cannot rewrite a tool call; ignoring the hook's updated input"
            );
        }
        if !matches!(o.decision, symposium_sdk::hook::Decision::Deny) {
            return command_code_hook_output_from_symposium("PreToolUse", event)
                .map(|v| serde_json::from_value(v).unwrap())
                .unwrap_or_default();
        }
        Self {
            hook_specific_output: Some(CommandCodeHookSpecificOutput {
                hook_event_name: "PreToolUse".into(),
                permission_decision: Some("deny".into()),
                permission_decision_reason: o.additional_context.clone(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        let h = self.hook_specific_output.as_ref();
        let denied = h.and_then(|h| h.permission_decision.as_deref()) == Some("deny");
        let (decision, context) = if denied {
            let reason = h
                .and_then(|h| h.permission_decision_reason.clone())
                .or_else(|| h.and_then(|h| h.additional_context.clone()));
            (symposium_sdk::hook::Decision::Deny, reason)
        } else {
            (
                symposium_sdk::hook::Decision::Allow,
                h.and_then(|h| h.additional_context.clone()),
            )
        };
        symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::new(
            decision, context, None,
        ))
    }
    fn to_hook_output(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── PostToolUse ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCodePostToolUseInput {
    pub hook_event_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: Value,
    #[serde(default)]
    pub tool_response: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandCodePostToolUseOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CommandCodeHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl AgentHookInput for CommandCodePostToolUseInput {
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

impl AgentHookOutput for CommandCodePostToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        command_code_hook_output_from_symposium("PostToolUse", event)
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
    fn to_hook_output(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── SessionStart ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCodeSessionStartInput {
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandCodeSessionStartOutput {
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<CommandCodeHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl AgentHookInput for CommandCodeSessionStartInput {
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
            source: None,
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

impl AgentHookOutput for CommandCodeSessionStartOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        command_code_hook_output_from_symposium("SessionStart", event)
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
    fn to_hook_output(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Plugin hooks see Command Code's own tool names and argument fields,
    /// as they do for every other agent.
    #[test]
    fn tool_calls_reach_plugins_as_command_code_sends_them() {
        let payload = r#"{
            "hook_event_name": "PreToolUse",
            "tool_name": "read_file",
            "tool_display_name": "READ",
            "tool_input": { "absolute_path": "/a/b" },
            "session_id": "s",
            "cwd": "/w"
        }"#;
        let input = CommandCodePreToolUseInput::parse_input(payload).unwrap();
        let symposium::InputEvent::PreToolUse(p) = input.to_symposium() else {
            panic!("wrong event")
        };
        assert_eq!(p.tool_name, "read_file");
        assert_eq!(p.tool_input, json!({ "absolute_path": "/a/b" }));
        assert_eq!(p.cwd.as_deref(), Some("/w"));
    }

    #[test]
    fn post_tool_use_carries_the_response() {
        let payload = r#"{
            "hook_event_name": "PostToolUse",
            "tool_name": "shell_command",
            "tool_input": { "command": "ls" },
            "tool_response": { "stdout": "ok" }
        }"#;
        let input = CommandCodePostToolUseInput::parse_input(payload).unwrap();
        let symposium::InputEvent::PostToolUse(p) = input.to_symposium() else {
            panic!("wrong event")
        };
        assert_eq!(p.tool_name, "shell_command");
        assert_eq!(p.tool_response, json!({ "stdout": "ok" }));
    }

    #[test]
    fn deny_is_written_where_command_code_reads_it() {
        let event = symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::new(
            symposium_sdk::hook::Decision::Deny,
            Some("not allowed".into()),
            None,
        ));
        let output = CommandCodePreToolUseOutput::from_symposium(&event).to_hook_output();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "not allowed"
        );
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }

    #[test]
    fn context_without_a_decision_is_forwarded_as_allow_with_context() {
        let event = symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::new(
            symposium_sdk::hook::Decision::Allow,
            Some("note".into()),
            None,
        ));
        let output = CommandCodePreToolUseOutput::from_symposium(&event).to_hook_output();
        assert_eq!(output["hookSpecificOutput"]["additionalContext"], "note");
        assert!(output["hookSpecificOutput"]["permissionDecision"].is_null());
    }
}
