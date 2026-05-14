use crate::hook_schema::{
    Agent, AgentHookEvent, ErasedAgentHookEvent, HookEvent, erase_agent_hook_event, symposium,
};

pub struct OpenCode;
impl Agent for OpenCode {
    fn event(&self, event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        Some(match event {
            HookEvent::PreToolUse => erase_agent_hook_event(OpenCodePreToolUseEvent),
            HookEvent::PostToolUse => erase_agent_hook_event(OpenCodePostToolUseEvent),
            HookEvent::UserPromptSubmit => erase_agent_hook_event(OpenCodeUserPromptSubmitEvent),
            HookEvent::SessionStart => erase_agent_hook_event(OpenCodeSessionStartEvent),
        })
    }
}

macro_rules! opencode_event {
    ($event:ident, $input:ty, $output:ty) => {
        struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

opencode_event!(
    OpenCodePreToolUseEvent,
    symposium::InputEvent,
    symposium::PreToolUseOutput
);
opencode_event!(
    OpenCodePostToolUseEvent,
    symposium::InputEvent,
    symposium::PostToolUseOutput
);
opencode_event!(
    OpenCodeUserPromptSubmitEvent,
    symposium::InputEvent,
    symposium::UserPromptSubmitOutput
);
opencode_event!(
    OpenCodeSessionStartEvent,
    symposium::InputEvent,
    symposium::SessionStartOutput
);
