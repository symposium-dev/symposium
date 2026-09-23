//! A `SessionStart` handler that puts a project's notes in front of the agent.
//!
//! ```bash
//! echo '{"SessionStart":{"session_id":null,"cwd":"/path/to/project"}}' \
//!   | cargo run --example inject_context
//! ```

use std::path::Path;
use std::process::ExitCode;

use symposium_sdk::hook::{HookHandler, SessionStartInput, SessionStartOutput, run};

struct InjectContext;

impl HookHandler for InjectContext {
    async fn session_start(&self, event: &SessionStartInput) -> anyhow::Result<SessionStartOutput> {
        let Some(cwd) = event.cwd.as_deref() else {
            return Ok(SessionStartOutput::default());
        };

        // A missing file is the ordinary case, not an error: most sessions
        // start somewhere with no notes to offer.
        match std::fs::read_to_string(Path::new(cwd).join("NOTES.md")) {
            Ok(notes) => Ok(SessionStartOutput::context(notes)),
            Err(_) => Ok(SessionStartOutput::default()),
        }
    }
}

fn main() -> ExitCode {
    run(InjectContext)
}
