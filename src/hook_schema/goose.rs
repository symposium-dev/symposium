use crate::hook_schema::{Agent, ErasedAgentHookEvent, HookEvent};

pub struct Goose;
impl Agent for Goose {
    fn event(&self, _event: HookEvent) -> Option<Box<dyn ErasedAgentHookEvent>> {
        None // Goose uses MCP extensions, not shell hooks
    }
}
