use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::{
    hook::{HookOutput, HookPayload, HookSubPayload, PreToolUsePayload, merge},
    hook_schema::{
        Agent, AgentHookEvent, AgentHookOutput, AgentHookPayload, erase_agent_hook_event,
    },
};

pub struct GeminiCode;
impl Agent for GeminiCode {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => Some(erase_agent_hook_event(GeminiPreToolUseEvent)),
            _ => None,
        }
    }
}
pub struct GeminiPreToolUseEvent;
impl AgentHookEvent for GeminiPreToolUseEvent {
    type Payload = GeminiBeforeToolPayload;
    type Output = GeminiBeforeToolUseOutput;

    fn parse_payload(&self, payload: &str) -> anyhow::Result<Self::Payload> {
        GeminiBeforeToolPayload::parse_payload(payload)
    }

    fn parse_output(&self, output: &[u8]) -> anyhow::Result<Self::Output> {
        GeminiBeforeToolUseOutput::parse_output(output)
    }

    fn from_hook_output(&self, output: &HookOutput) -> anyhow::Result<Self::Output> {
        GeminiBeforeToolUseOutput::from_hook_output(output)
    }

    fn merge_outputs(first: Self::Output, second: Self::Output) -> Self::Output {
        let mut first = serde_json::to_value(first).unwrap();
        let second = serde_json::to_value(second).unwrap();
        merge(&mut first, second);
        serde_json::from_value(first).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiHookCommonPayload {
    pub(crate) hook_event_name: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) transcript_path: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiBeforeToolPayload {
    #[serde(flatten)]
    pub common_payload: GeminiHookCommonPayload,
    pub(crate) tool_name: String,
    pub(crate) tool_input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mcp_context: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) original_request_name: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AgentHookPayload for GeminiBeforeToolPayload {
    fn parse_payload(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }

    fn to_hook_payload(&self) -> HookPayload {
        let sub_payload = HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: self.tool_name.clone(),
        });
        // Forward Gemini fields to the internal HookPayload.rest so plugin
        // hooks can access `tool_input`, `mcp_context`, and other metadata.
        let mut rest = self.rest.clone();
        rest.insert("tool_input".to_string(), self.tool_input.clone());
        if let Some(ref mcp) = self.mcp_context {
            rest.insert("mcp_context".to_string(), mcp.clone());
        }
        if let Some(ref name) = self.original_request_name {
            rest.insert("original_request_name".to_string(), serde_json::Value::String(name.clone()));
        }
        if let Some(ref s) = self.common_payload.session_id {
            rest.insert("session_id".to_string(), serde_json::Value::String(s.clone()));
        }
        if let Some(ref c) = self.common_payload.cwd {
            rest.insert("cwd".to_string(), serde_json::Value::String(c.clone()));
        }
        if let Some(ref t) = self.common_payload.transcript_path {
            rest.insert("transcript_path".to_string(), serde_json::Value::String(t.clone()));
        }
        if let Some(ref ts) = self.common_payload.timestamp {
            rest.insert("timestamp".to_string(), serde_json::Value::String(ts.clone()));
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
pub struct GeminiBeforeToolUseOutput {
    #[serde(rename = "decision", skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "systemMessage", skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "suppressOutput", skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub continue_: Option<bool>,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<InnerHookSpecificOutput>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerHookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(rename = "tool_input", skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl GeminiBeforeToolUseOutput {
    pub fn new() -> Self {
        Self {
            decision: None,
            reason: None,
            system_message: None,
            suppress_output: None,
            continue_: None,
            stop_reason: None,
            hook_specific_output: Some(InnerHookSpecificOutput {
                hook_event_name: "BeforeTool".to_string(),
                additional_context: None,
                tool_input: None,
                rest: serde_json::Map::new(),
            }),
            rest: serde_json::Map::new(),
        }
    }
}

impl AgentHookOutput for GeminiBeforeToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(serde_json::from_slice(output)?)
    }

    fn from_hook_output(payload: &HookOutput) -> anyhow::Result<Self> {
        let Some(hook_specific_output) = &payload.hook_specific_output else {
            return Err(anyhow!("missing hook specific output"));
        };
        if hook_specific_output.hook_event_name != "BeforeTool" {
            return Err(anyhow!("unexpected hook event name"));
        }

        // Convert the entire HookOutput to JSON and deserialize into the Gemini output
        let value = serde_json::to_value(payload).unwrap();
        Ok(serde_json::from_value(value).unwrap())
    }

    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
