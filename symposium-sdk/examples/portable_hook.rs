//! One handler that can run directly under Claude Code, Codex, or Kiro.
//!
//! ```bash
//! echo '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test"}}' \
//!   | cargo run -p symposium-sdk --example portable_hook
//! ```
//!
//! Pass `--agent=codex` (after `--` when using `cargo run`) to select an
//! adapter explicitly, or `--agent=symposium` for Symposium's tagged events.
//! Without a flag, the SDK tries Claude Code, Codex, then Kiro.

use std::process::ExitCode;

use symposium_sdk::hook::{
    HookHandler, PreToolUseInput, PreToolUseOutput, SessionStartInput, SessionStartOutput,
    hook_main,
};

struct PortableHook;

impl HookHandler for PortableHook {
    async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
        // Inspect the command argument instead of relying on an agent's tool
        // name. This is an example policy, not a complete shell-command filter.
        match event
            .tool_input
            .get("command")
            .and_then(|value| value.as_str())
        {
            Some("rm -rf /") => Ok(PreToolUseOutput::deny(
                "Refusing to remove the root directory.",
            )),
            Some("cargo test") => {
                let mut input = event.tool_input.clone();
                input["command"] = "cargo test --locked".into();
                Ok(PreToolUseOutput::with_updated_input(input))
            }
            _ => Ok(PreToolUseOutput::context(
                "Review tool arguments before execution.",
            )),
        }
    }

    async fn session_start(
        &self,
        _event: &SessionStartInput,
    ) -> anyhow::Result<SessionStartOutput> {
        Ok(SessionStartOutput::context(
            "Prefer small changes and run relevant tests.",
        ))
    }
}

fn main() -> ExitCode {
    hook_main(PortableHook)
}
