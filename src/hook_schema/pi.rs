//! The Pi extension uses the SDK's event-specific JSON fields directly.
//! The command-line event name supplies the event tag.

use crate::hook_schema::symposium::*;
use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, ErasedAgentHookEvent, HookEvent,
    erase_agent_hook_event,
};

pub struct Pi;

impl Agent for Pi {
    fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        Some(match event {
            HookEvent::PreToolUse => erase_agent_hook_event(PreToolUse),
            HookEvent::PostToolUse => erase_agent_hook_event(PostToolUse),
            HookEvent::UserPromptSubmit => erase_agent_hook_event(UserPromptSubmit),
            HookEvent::SessionStart => erase_agent_hook_event(SessionStart),
            HookEvent::Stop => erase_agent_hook_event(Stop),
            _ => return None,
        })
    }
}

macro_rules! pi_event {
    ($event:ident, $input:ident, $output:ident) => {
        struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
        impl AgentHookInput for $input {
            fn parse_input(payload: &str) -> anyhow::Result<Self> {
                Ok(serde_json::from_str(payload)?)
            }
            fn to_symposium(&self) -> InputEvent {
                InputEvent::$event(self.clone())
            }
            fn from_symposium(event: &InputEvent) -> Self {
                match event {
                    InputEvent::$event(input) => input.clone(),
                    _ => unreachable!("Pi input event mismatch"),
                }
            }
            fn to_string(&self) -> anyhow::Result<String> {
                Ok(serde_json::to_string(self)?)
            }
            fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
                self
            }
        }
        impl AgentHookOutput for $output {
            fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
                if output.iter().all(u8::is_ascii_whitespace) {
                    return Ok(Self::default());
                }
                Ok(serde_json::from_slice(output)?)
            }
            fn from_symposium(event: &OutputEvent) -> Self {
                match event {
                    OutputEvent::$event(output) => output.clone(),
                    _ => unreachable!("Pi output event mismatch"),
                }
            }
            fn to_symposium(&self) -> OutputEvent {
                OutputEvent::$event(self.clone())
            }
            fn to_hook_output(&self) -> serde_json::Value {
                serde_json::to_value(self).expect("Pi output serialization")
            }
            fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
                self
            }
        }
    };
}

pi_event!(PreToolUse, PreToolUseInput, PreToolUseOutput);
pi_event!(PostToolUse, PostToolUseInput, PostToolUseOutput);
pi_event!(
    UserPromptSubmit,
    UserPromptSubmitInput,
    UserPromptSubmitOutput
);
pi_event!(SessionStart, SessionStartInput, SessionStartOutput);
pi_event!(Stop, StopInput, StopOutput);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_schema::HookAgent;

    #[test]
    fn pi_events_convert_sdk_payloads_and_outputs() {
        for (event, input) in [
            (
                HookEvent::SessionStart,
                serde_json::json!({"cwd": "/project", "session_id": "s1"}),
            ),
            (
                HookEvent::UserPromptSubmit,
                serde_json::json!({"prompt": "hello"}),
            ),
            (
                HookEvent::PreToolUse,
                serde_json::json!({"tool_name": "bash", "tool_input": {"command": "ls"}}),
            ),
            (
                HookEvent::PostToolUse,
                serde_json::json!({"tool_name": "bash", "tool_input": {}, "tool_response": {"isError": false}}),
            ),
            (HookEvent::Stop, serde_json::json!({"session_id": "s1"})),
        ] {
            let handler = crate::hook_schema::agent_event(HookAgent::Pi, event).unwrap();
            let parsed = handler.parse_input(&input.to_string()).unwrap();
            assert_eq!(parsed.to_symposium().event(), event);
            let translated = handler.translate_input(&parsed.to_symposium());
            assert_eq!(parsed.to_string().unwrap(), translated.to_string().unwrap());
            let output = OutputEvent::with_context(event, "context".into());
            let native = handler.translate_output(&output).to_hook_output();
            assert_eq!(native["additionalContext"], "context");
            let roundtrip = handler
                .parse_output(&serde_json::to_vec(&native).unwrap())
                .unwrap();
            assert_eq!(
                roundtrip.to_symposium().additional_context(),
                Some("context")
            );
            assert!(
                handler
                    .parse_output(b"")
                    .unwrap()
                    .to_symposium()
                    .additional_context()
                    .is_none()
            );
        }
    }

    #[test]
    fn pi_pre_tool_output_keeps_denials_and_input_changes() {
        let handler = Pi.event(HookEvent::PreToolUse).unwrap();
        let mut output = PreToolUseOutput::deny("blocked");
        output.updated_input = Some(serde_json::json!({"command": "pwd"}));
        let native = handler
            .translate_output(&OutputEvent::PreToolUse(output))
            .to_hook_output();
        assert_eq!(native["decision"], "deny");
        assert_eq!(native["updatedInput"]["command"], "pwd");
    }
}
