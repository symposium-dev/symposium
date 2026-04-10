use crate::hook_schema::{Agent, ErasedAgentHookEvent, HookEvent};

pub struct OpenCode;
impl Agent for OpenCode {
    fn event(&self, _event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        None // OpenCode uses JS/TS plugins, not shell hooks
    }
}
