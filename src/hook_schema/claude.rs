use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::{
    hook::{HookOutput, HookPayload, HookSubPayload, PreToolUsePayload, merge},
    hook_schema::{
        Agent, AgentHookEvent, AgentHookOutput, AgentHookPayload, erase_agent_hook_event,
    },
};

pub struct ClaudeCode;
impl Agent for ClaudeCode {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(ClaudePreToolUseEvent)),
            _ => None,
        }
    }
}
pub struct ClaudePreToolUseEvent;
impl AgentHookEvent for ClaudePreToolUseEvent {
    type Payload = ClaudeCodePreToolUsePayload;
    type Output = ClaudeCodePreToolUseOutput;

    fn parse_payload(&self, payload: &str) -> anyhow::Result<Self::Payload> {
        ClaudeCodePreToolUsePayload::parse_payload(payload)
    }

    fn parse_output(&self, output: &[u8]) -> anyhow::Result<Self::Output> {
        ClaudeCodePreToolUseOutput::parse_output(output)
    }

    fn from_hook_output(&self, output: &HookOutput) -> anyhow::Result<Self::Output> {
        ClaudeCodePreToolUseOutput::from_hook_output(output)
    }

    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output {
        let mut first = serde_json::to_value(first).unwrap();
        let second = serde_json::to_value(second).unwrap();
        merge(&mut first, second);
        serde_json::from_value(first).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeHookCommonPayload {
    pub(crate) hook_event_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodePreToolUsePayload {
    #[serde(flatten)]
    pub common_payload: ClaudeCodeHookCommonPayload,
    pub(crate) tool_name: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookPayload for ClaudeCodePreToolUsePayload {
    fn parse_payload(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    fn to_hook_payload(&self) -> HookPayload {
        let sub_payload = HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: self.tool_name.clone(),
        });
        HookPayload {
            sub_payload,
            rest: self.rest.clone(),
        }
    }

    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodePreToolUseOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub do_continue: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,

    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<ClaudeHookSpecificOutput>,

    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,

    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(rename = "permissionDecisionReason", skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<String>,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,

    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl ClaudeCodePreToolUseOutput {
    pub fn new() -> Self {
        Self {
            do_continue: None,
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            hook_specific_output: Some(ClaudeHookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                permission_decision: None,
                permission_decision_reason: None,
                updated_input: None,
                additional_context: None,
                rest: serde_json::Map::new(),
            }),
            rest: serde_json::Map::new(),
        }
    }
}

impl AgentHookOutput for ClaudeCodePreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(serde_json::from_slice(output)?)
    }

    fn from_hook_output(payload: &HookOutput) -> anyhow::Result<Self> {
        let mut out = Self::new();
        out.rest = payload.rest.clone();
        if let Some(hook_specific) = &payload.hook_specific_output {
            if hook_specific.hook_event_name != "PreToolUse" {
                return Err(anyhow!(
                    "unexpected hook event name: {}",
                    hook_specific.hook_event_name
                ));
            }
            out.hook_specific_output = Some(ClaudeHookSpecificOutput {
                hook_event_name: hook_specific.hook_event_name.clone(),
                permission_decision: None,
                permission_decision_reason: None,
                updated_input: hook_specific.updated_input.clone(),
                additional_context: hook_specific.additional_context.clone(),
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
