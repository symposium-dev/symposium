//! Symposium ACP Agent
//!
//! A standalone agent binary that combines the Symposium component chain with
//! a downstream agent. This is the "I am the agent" mode - Zed or other editors
//! spawn this binary directly, and it provides an enriched agent experience.
//!
//! Usage:
//!   symposium-acp-agent [OPTIONS] -- <agent-command> [agent-args...]
//!
//! Example:
//!   symposium-acp-agent -- npx -y @zed-industries/claude-code-acp

use anyhow::Result;
use clap::Parser;
use sacp::Component;
use sacp_tokio::AcpAgent;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-agent")]
#[command(about = "Symposium-enriched ACP agent")]
#[command(
    long_about = "Combines the Symposium component chain with a downstream agent.\n\n\
                  This binary acts as an enriched agent - editors spawn it directly,\n\
                  and it provides Symposium's capabilities on top of the underlying agent."
)]
struct Cli {
    /// Disable Sparkle integration
    #[arg(long, default_value = "false")]
    no_sparkle: bool,

    /// Disable the crate researcher
    #[arg(long, default_value = "false")]
    no_crate_researcher: bool,

    /// Enable trace logging to the specified directory.
    /// Traces are written as timestamped .jsons files viewable with sacp-trace-viewer.
    #[arg(long)]
    trace_dir: Option<PathBuf>,

    /// Enable logging to stderr at the specified level (error, warn, info, debug, trace).
    #[arg(long)]
    log: Option<tracing::Level>,

    /// The agent command and arguments (e.g., npx -y @zed-industries/claude-code-acp)
    #[arg(last = true, required = true, num_args = 1..)]
    agent: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging if requested
    if let Some(level) = cli.log {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(level.to_string()))
            .with_writer(std::io::stderr)
            .init();
    }

    // Build a shell command string from the args
    let agent: AcpAgent = AcpAgent::from_args(&cli.agent)?;
    tracing::debug!("agent: {:?}", agent.server());

    // Run Symposium with the agent as the downstream component
    let mut symposium = symposium_acp_proxy::Symposium::new()
        .sparkle(!cli.no_sparkle)
        .crate_sources_proxy(!cli.no_crate_researcher);

    if let Some(trace_dir) = cli.trace_dir {
        symposium = symposium.trace_dir(trace_dir);
    }

    symposium
        .with_agent(agent)
        .serve(sacp_tokio::Stdio::new())
        .await?;

    Ok(())
}
