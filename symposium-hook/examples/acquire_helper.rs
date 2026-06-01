//! Example: acquire a helper binary (`cargo-bp`) and invoke it at session start.
//!
//! Demonstrates how a hook handler can use `symposium-install` to lazily
//! install a cargo binary and run it to produce dynamic context.

use std::process::ExitCode;

use symposium_hook::{HookHandler, SessionStartInput, SessionStartOutput, run};
use symposium_install::{CargoSource, InstallContext, Source, acquire_source};

struct BpContextHook {
    install_ctx: InstallContext,
}

impl BpContextHook {
    fn new() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
            .join("symposium");
        Self {
            install_ctx: InstallContext::new(cache_dir),
        }
    }
}

impl HookHandler for BpContextHook {
    async fn session_start(
        &self,
        _event: &SessionStartInput,
    ) -> anyhow::Result<SessionStartOutput> {
        let source = Source::Cargo(CargoSource::new("cargo-bp"));

        let acquired = acquire_source(&self.install_ctx, &source, None).await?;
        let binary_path = acquired
            .executable()
            .ok_or_else(|| anyhow::anyhow!("could not determine binary name for cargo-bp"))?;

        let output = tokio::process::Command::new(&binary_path)
            .args(["bp", "list", "-N"])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("cargo-bp failed: {stderr}");
        }

        let context = String::from_utf8(output.stdout)?;
        Ok(SessionStartOutput::context(context))
    }
}

fn main() -> ExitCode {
    run(BpContextHook::new())
}
