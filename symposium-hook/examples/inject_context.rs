use std::process::ExitCode;
use symposium_hook::{HookHandler, SessionStartInput, SessionStartOutput, run};

struct InjectContext;

impl HookHandler for InjectContext {
    fn session_start(&self, _event: &SessionStartInput) -> anyhow::Result<SessionStartOutput> {
        Ok(SessionStartOutput::context(
            "This project uses tokio 1.x for async. Prefer spawn over block_on.",
        ))
    }
}

fn main() -> ExitCode {
    run(InjectContext)
}
