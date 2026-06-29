//! Canonical symposium hook types — the "lingua franca" between agents.
//!
//! Each agent module converts to/from these types. Builtin dispatch
//! operates entirely on these types.
//!
//! Type definitions are owned by `symposium-sdk` so that hook authors can
//! depend on them without pulling in the full symposium binary. This module
//! re-exports those types.

// Re-export wire types from the SDK crate.
pub use symposium_sdk::hook::{
    Input as InputEvent, Output as OutputEvent, PostToolUseInput, PostToolUseOutput,
    PreToolUseInput, PreToolUseOutput, SessionStartInput, SessionStartOutput, StopInput,
    StopOutput, UserPromptSubmitInput, UserPromptSubmitOutput,
};

// ── AgentHookInput for InputEvent ────────────────────────────────────────
// Allows symposium-format plugins to receive canonical InputEvent JSON.

impl super::AgentHookInput for InputEvent {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> InputEvent {
        self.clone()
    }
    fn from_symposium(event: &InputEvent) -> Self {
        event.clone()
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_matchers_use_regex_against_tool_name() {
        let input = InputEvent::PreToolUse(PreToolUseInput::new(
            "mcp__filesystem__read".to_string(),
            serde_json::Value::Null,
            None,
            None,
        ));

        assert!(input.matches_matcher("mcp__.*"));
        assert!(input.matches_matcher("^mcp__filesystem__read$"));
        assert!(!input.matches_matcher("^Bash$"));
        assert!(!input.matches_matcher("^filesystem$"));
    }

    #[test]
    fn invalid_regex_matchers_do_not_match() {
        let input = InputEvent::PostToolUse(PostToolUseInput::new(
            "Bash".to_string(),
            serde_json::Value::Null,
            serde_json::Value::Null,
            None,
            None,
        ));

        assert!(!input.matches_matcher("("));
    }

    #[test]
    fn wildcard_matches_everything() {
        let input = InputEvent::SessionStart(SessionStartInput::new(None, None));

        assert!(input.matches_matcher("*"));
    }
}
