use std::process::ExitCode;
use symposium_hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};

struct BlockDestructive;

impl HookHandler for BlockDestructive {
    fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
        if event.tool_name == "Bash" {
            if let Some(cmd) = event.tool_input.get("command").and_then(|v| v.as_str()) {
                if cmd.contains("rm -rf") {
                    return Ok(PreToolUseOutput::deny(
                        "Destructive rm -rf commands are not allowed",
                    ));
                }
            }
        }
        Ok(PreToolUseOutput::default())
    }
}

fn main() -> ExitCode {
    run(BlockDestructive)
}
