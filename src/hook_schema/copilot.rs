use serde::{Deserialize, Serialize};

use crate::{
    hook::{HookOutput, HookPayload, HookSubPayload, PreToolUsePayload, merge},
    hook_schema::{
        Agent, AgentHookEvent, AgentHookOutput, AgentHookPayload, erase_agent_hook_event,
    },
};

pub struct Copilot;
impl Agent for Copilot {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(CopilotPreToolUseEvent)),
            _ => None,
        }
    }
}

pub struct CopilotPreToolUseEvent;
impl AgentHookEvent for CopilotPreToolUseEvent {
    type Payload = CopilotPreToolUsePayload;
    type Output = CopilotPreToolUseOutput;

    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output {
        let mut first = serde_json::to_value(first).unwrap();
        let second = serde_json::to_value(second).unwrap();
        merge(&mut first, second);
        serde_json::from_value(first).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotPreToolUsePayload {
    #[serde(rename = "timestamp")]
    pub timestamp: Option<i64>,
    #[serde(rename = "cwd")]
    pub cwd: Option<String>,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "toolArgs")]
    pub tool_args: serde_json::Value,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookPayload for CopilotPreToolUsePayload {
    fn parse_payload(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    fn to_hook_payload(&self) -> HookPayload {
        let sub_payload = HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: self.tool_name.clone(),
        });

        let mut rest = self.rest.clone();
        // Copilot sends toolArgs as a JSON string; parse it into a Value
        // so downstream code can inspect structured fields.
        let tool_args = match &self.tool_args {
            serde_json::Value::String(s) => {
                serde_json::from_str(s).unwrap_or_else(|_| self.tool_args.clone())
            }
            other => other.clone(),
        };
        rest.insert("tool_args".to_string(), tool_args);
        if let Some(ts) = self.timestamp {
            rest.insert("timestamp".to_string(), serde_json::Value::Number(ts.into()));
        }
        if let Some(ref c) = self.cwd {
            rest.insert("cwd".to_string(), serde_json::Value::String(c.clone()));
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
pub struct CopilotPreToolUseOutput {
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(rename = "permissionDecisionReason", skip_serializing_if = "Option::is_none")]
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

impl Default for CopilotPreToolUseOutput {
    fn default() -> Self {
        Self {
            permission_decision: None,
            permission_decision_reason: None,
            modified_args: None,
            additional_context: None,
            suppress_output: None,
            rest: serde_json::Map::new(),
        }
    }
}

impl AgentHookOutput for CopilotPreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(serde_json::from_slice(output)?)
    }

    fn from_hook_output(payload: &HookOutput) -> anyhow::Result<Self> {
        // Map our internal HookOutput to the Copilot output shape.
        let mut out = CopilotPreToolUseOutput::default();
        if let Some(ref hook_specific) = payload.hook_specific_output {
            out.additional_context = hook_specific.additional_context.clone();
            // merge any hookSpecific.rest into out.rest
            for (k, v) in hook_specific.rest.iter() {
                out.rest.insert(k.clone(), v.clone());
            }
        }
        // copy other top-level rest fields
        for (k, v) in payload.rest.iter() {
            out.rest.insert(k.clone(), v.clone());
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
