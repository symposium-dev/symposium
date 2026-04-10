use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::{
    hook::{HookOutput, HookPayload, HookSubPayload, PreToolUsePayload, merge},
    hook_schema::{
        Agent, AgentHookEvent, AgentHookOutput, AgentHookPayload, erase_agent_hook_event,
    },
};

pub struct Codex;
impl Agent for Codex {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(CodexPreToolUseEvent)),
            _ => None,
        }
    }
}

pub struct CodexPreToolUseEvent;
impl AgentHookEvent for CodexPreToolUseEvent {
    type Payload = CodexPreToolUsePayload;
    type Output = CodexPreToolUseOutput;

    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output {
        let mut first = serde_json::to_value(first).unwrap();
        let second = serde_json::to_value(second).unwrap();
        merge(&mut first, second);
        serde_json::from_value(first).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPreToolUsePayload {
    pub(crate) hook_event_name: String,
    pub(crate) tool_name: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) turn_id: Option<String>,
    #[serde(default)]
    pub(crate) tool_use_id: Option<String>,
    #[serde(default)]
    pub(crate) tool_input: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookPayload for CodexPreToolUsePayload {
    fn parse_payload(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    fn to_hook_payload(&self) -> HookPayload {
        let sub_payload = HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: self.tool_name.clone(),
        });

        let mut rest = self.rest.clone();
        if let Some(ref sid) = self.session_id {
            rest.insert("session_id".to_string(), serde_json::Value::String(sid.clone()));
        }
        if let Some(ref c) = self.cwd {
            rest.insert("cwd".to_string(), serde_json::Value::String(c.clone()));
        }
        if let Some(ref m) = self.model {
            rest.insert("model".to_string(), serde_json::Value::String(m.clone()));
        }
        if let Some(ref tid) = self.turn_id {
            rest.insert("turn_id".to_string(), serde_json::Value::String(tid.clone()));
        }
        if let Some(ref tuid) = self.tool_use_id {
            rest.insert("tool_use_id".to_string(), serde_json::Value::String(tuid.clone()));
        }
        if let Some(ref ti) = self.tool_input {
            rest.insert("tool_input".to_string(), ti.clone());
        }

        HookPayload { sub_payload, rest }
    }

    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexHookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,

    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(rename = "permissionDecisionReason", skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,

    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for CodexPreToolUseOutput {
    fn default() -> Self {
        Self {
            decision: None,
            reason: None,
            do_continue: None,
            stop_reason: None,
            system_message: None,
            hook_specific_output: Some(CodexHookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                additional_context: None,
                permission_decision: None,
                permission_decision_reason: None,
                rest: serde_json::Map::new(),
            }),
            rest: serde_json::Map::new(),
        }
    }
}

impl AgentHookOutput for CodexPreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(serde_json::from_slice(output)?)
    }

    fn from_hook_output(payload: &HookOutput) -> anyhow::Result<Self> {
        let mut out = Self::default();
        out.rest = payload.rest.clone();
        if let Some(hook_specific) = &payload.hook_specific_output {
            if hook_specific.hook_event_name != "PreToolUse" {
                return Err(anyhow!(
                    "unexpected hook event name: {}",
                    hook_specific.hook_event_name
                ));
            }
            out.hook_specific_output = Some(CodexHookSpecificOutput {
                hook_event_name: hook_specific.hook_event_name.clone(),
                additional_context: hook_specific.additional_context.clone(),
                permission_decision: None,
                permission_decision_reason: None,
                rest: hook_specific.rest.clone(),
            });
        }
        Ok(out)
    }

    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
