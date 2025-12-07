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

use std::ffi::OsString;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use clap::Parser;
use sacp_conductor::Conductor;
use sacp_tokio::AcpAgent;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-agent")]
#[command(about = "Symposium-enriched ACP agent")]
#[command(
    long_about = "Combines the Symposium component chain with a downstream agent.\n\n\
                  This binary acts as an enriched agent - editors spawn it directly,\n\
                  and it provides Symposium's capabilities on top of the underlying agent."
)]
struct Cli {
    /// Enable Sparkle integration
    #[arg(long, default_value = "true")]
    sparkle: bool,

    /// Redirect tracing output to a file instead of stderr
    #[arg(long)]
    log_to: Option<PathBuf>,

    /// Set tracing filter (e.g., "info", "debug", "symposium=trace")
    #[arg(long)]
    log: Option<String>,

    /// The agent command and arguments (e.g., npx -y @zed-industries/claude-code-acp)
    #[arg(last = true, required = true, num_args = 1..)]
    agent: Vec<OsString>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    init_tracing(&cli)?;

    tracing::info!("Starting Symposium ACP Agent");
    tracing::info!("Downstream agent: {:?}", cli.agent);

    // Build the conductor with Symposium components + the agent
    let conductor = build_conductor(&cli)?;

    // Run the conductor over stdio
    conductor.run(sacp_tokio::Stdio::new()).await?;

    Ok(())
}

fn init_tracing(cli: &Cli) -> Result<()> {
    let filter = if let Some(ref log_filter) = cli.log {
        tracing_subscriber::EnvFilter::new(log_filter)
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    if let Some(ref log_file) = cli.log_to {
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(log_file)?;
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter(filter)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .init();
    }

    Ok(())
}

/// Quote a string for shell if it contains special characters
fn shell_quote(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || "\"'\\$`!".contains(c)) {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    } else {
        s.to_string()
    }
}

/// Convert OsString args to a shell command string
fn args_to_shell_command(args: &[OsString]) -> Result<String> {
    let parts: Result<Vec<String>, _> = args
        .iter()
        .map(|s| {
            s.to_str()
                .ok_or_else(|| anyhow::anyhow!("Agent command contains invalid UTF-8"))
                .map(shell_quote)
        })
        .collect();
    Ok(parts?.join(" "))
}

fn build_conductor(cli: &Cli) -> Result<Conductor> {
    // Build a shell command string from the args
    let agent_command = args_to_shell_command(&cli.agent)?;
    let sparkle_enabled = cli.sparkle;

    let conductor = Conductor::new(
        "symposium-agent".to_string(),
        move |init_req| {
            let agent_command = agent_command.clone();
            async move {
                tracing::info!("Building Symposium agent chain");

                let mut components = vec![];

                // Add the Symposium proxy components
                components.push(sacp::DynComponent::new(
                    symposium_crate_sources_proxy::CrateSourcesProxy {},
                ));

                if sparkle_enabled {
                    components.push(sacp::DynComponent::new(sparkle::SparkleComponent::new()));
                }

                // Parse and add the downstream agent as the final component
                let agent = AcpAgent::from_str(&agent_command)?;
                components.push(sacp::DynComponent::new(agent));

                Ok((init_req, components))
            }
        },
        Default::default(),
    );

    Ok(conductor)
}
