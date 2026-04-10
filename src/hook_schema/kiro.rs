use serde::{Deserialize, Serialize};

use crate::{
    hook::{HookOutput, HookPayload, HookSubPayload, PreToolUsePayload, merge},
    hook_schema::{
        Agent, AgentHookEvent, AgentHookOutput, AgentHookPayload, erase_agent_hook_event,
    },
};

pub struct Kiro;
impl Agent for Kiro {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(KiroPreToolUseEvent)),
            _ => None,
        }
    }
}

pub struct KiroPreToolUseEvent;
impl AgentHookEvent for KiroPreToolUseEvent {
    type Payload = KiroPreToolUsePayload;
    type Output = KiroPreToolUseOutput;

    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output {
        let mut first = serde_json::to_value(first).unwrap();
        let second = serde_json::to_value(second).unwrap();
        merge(&mut first, second);
        serde_json::from_value(first).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroPreToolUsePayload {
    pub(crate) hook_event_name: String,
    pub(crate) tool_name: String,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) tool_input: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookPayload for KiroPreToolUsePayload {
    fn parse_payload(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    fn to_hook_payload(&self) -> HookPayload {
        let sub_payload = HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: self.tool_name.clone(),
        });

        let mut rest = self.rest.clone();
        if let Some(ref c) = self.cwd {
            rest.insert("cwd".to_string(), serde_json::Value::String(c.clone()));
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
pub struct KiroPreToolUseOutput {
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl Default for KiroPreToolUseOutput {
    fn default() -> Self {
        Self {
            additional_context: None,
            rest: serde_json::Map::new(),
        }
    }
}

impl AgentHookOutput for KiroPreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }

    fn from_hook_output(payload: &HookOutput) -> anyhow::Result<Self> {
        let mut out = KiroPreToolUseOutput::default();
        if let Some(ref hook_specific) = payload.hook_specific_output {
            out.additional_context = hook_specific.additional_context.clone();
            for (k, v) in hook_specific.rest.iter() {
                out.rest.insert(k.clone(), v.clone());
            }
        }
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
