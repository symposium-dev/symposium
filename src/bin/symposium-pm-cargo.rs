//! The cargo package manager as a standalone PM binary.
//!
//! Speaks the SDK's package-manager protocol on stdin/stdout. It lives in this
//! crate rather than in `symposium-pm-cargo` so that `cargo install symposium`
//! installs it alongside `cargo-agents`. A PM the user has to install
//! separately would be a PM that is usually missing.
//!
//! The `CargoPm` here is the same type Symposium uses in-process; the only
//! difference is that `initialize` supplies the workspace instead of the
//! constructor.

use symposium_pm_cargo::CargoPm;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Diagnostics go to stderr: stdout is the protocol.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SYMPOSIUM_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    symposium_sdk::pm::server::run(CargoPm::default()).await
}
