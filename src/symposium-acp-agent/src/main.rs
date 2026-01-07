//! Symposium ACP Agent
//!
//! A unified binary for running Symposium-enriched ACP agents and proxies.
//!
//! ## Commands
//!
//! ### act-as-agent
//! Wraps a downstream agent with Symposium's proxy chain:
//! ```bash
//! symposium-acp-agent act-as-agent --proxy sparkle --proxy ferris -- npx -y @anthropic-ai/claude-code-acp
//! ```
//!
//! ### act-as-proxy
//! Sits between an editor and an existing agent as a pure proxy:
//! ```bash
//! symposium-acp-agent act-as-proxy --proxy sparkle --proxy ferris
//! ```
//!
//! ### eliza
//! Runs the built-in Eliza test agent:
//! ```bash
//! symposium-acp-agent eliza
//! ```
//!
//! ### vscodelm
//! Runs as a VS Code Language Model Provider backend:
//! ```bash
//! symposium-acp-agent vscodelm
//! ```
//!
//! ## Proxy Configuration
//!
//! Use `--proxy <name>` to specify extensions. Order matters - proxies are
//! chained in the order specified.
//!
//! Known proxies: sparkle, ferris, cargo
//!
//! Special value `defaults` expands to all known proxies:
//! ```bash
//! --proxy defaults           # equivalent to: --proxy sparkle --proxy ferris --proxy cargo
//! --proxy foo --proxy defaults --proxy bar  # foo, sparkle, ferris, cargo, bar
//! ```

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use sacp::Component;
use sacp_tokio::AcpAgent;
use std::path::PathBuf;

mod symposium;
pub mod vscodelm;

use symposium::{Symposium, SymposiumConfig, KNOWN_PROXIES};

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-agent")]
#[command(about = "Symposium-enriched ACP agent and proxy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Wrap a downstream agent with Symposium's proxy chain
    ActAsAgent {
        #[command(flatten)]
        proxy_opts: ProxyOptions,

        /// The agent command and arguments (e.g., npx -y @anthropic-ai/claude-code-acp)
        #[arg(last = true, required = true, num_args = 1..)]
        agent: Vec<String>,
    },

    /// Run as a proxy between editor and an existing agent
    ActAsProxy {
        #[command(flatten)]
        proxy_opts: ProxyOptions,
    },

    /// Run the built-in Eliza agent (useful for testing)
    Eliza,

    /// Run as a VS Code Language Model Provider backend
    Vscodelm {
        /// Enable trace logging to the specified directory
        #[arg(long)]
        trace_dir: Option<PathBuf>,
    },
}

/// Shared proxy configuration options
#[derive(Args, Debug)]
struct ProxyOptions {
    /// Extension proxy to include in the chain (can be specified multiple times).
    /// Order matters - proxies are chained in the order specified.
    ///
    /// Known proxies: sparkle, ferris, cargo
    ///
    /// Special value "defaults" expands to all known proxies.
    #[arg(long = "proxy", value_name = "NAME")]
    proxies: Vec<String>,

    /// Enable trace logging to the specified directory.
    /// Traces are written as timestamped .jsons files viewable with sacp-trace-viewer.
    #[arg(long)]
    trace_dir: Option<PathBuf>,

    /// Enable logging to stderr. Accepts a level (error, warn, info, debug, trace)
    /// or a RUST_LOG-style filter string (e.g., "sacp=debug,symposium=trace").
    #[arg(long)]
    log: Option<String>,
}

impl ProxyOptions {
    /// Expand proxy names, handling "defaults" expansion.
    /// Returns an error if any proxy name is unknown.
    fn expand_proxy_names(&self) -> Result<Vec<String>> {
        let mut result = Vec::new();

        for name in &self.proxies {
            if name == "defaults" {
                // Expand "defaults" to all known proxies
                result.extend(KNOWN_PROXIES.iter().map(|s| s.to_string()));
            } else if KNOWN_PROXIES.contains(&name.as_str()) {
                result.push(name.clone());
            } else {
                anyhow::bail!(
                    "Unknown proxy name: '{}'. Known proxies: {}, defaults",
                    name,
                    KNOWN_PROXIES.join(", ")
                );
            }
        }

        Ok(result)
    }

    /// Build a SymposiumConfig from these options.
    fn into_config(self) -> Result<SymposiumConfig> {
        let proxy_names = self.expand_proxy_names()?;
        let mut config = SymposiumConfig::from_proxy_names(proxy_names);

        if let Some(trace_dir) = self.trace_dir {
            config = config.trace_dir(trace_dir);
        }

        Ok(config)
    }

    /// Set up logging if requested.
    fn setup_logging(&self) {
        if let Some(filter) = &self.log {
            use tracing_subscriber::EnvFilter;
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(filter))
                .with_writer(std::io::stderr)
                .init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::ActAsAgent { proxy_opts, agent } => {
            proxy_opts.setup_logging();

            let config = proxy_opts.into_config()?;
            let agent = AcpAgent::from_args(&agent)?;

            tracing::debug!(
                "Starting in agent mode with downstream: {:?}",
                agent.server()
            );

            Symposium::new(config)
                .with_agent(agent)
                .serve(sacp_tokio::Stdio::new())
                .await?;
        }

        Command::ActAsProxy { proxy_opts } => {
            proxy_opts.setup_logging();

            let config = proxy_opts.into_config()?;

            tracing::debug!("Starting in proxy mode");

            Symposium::new(config)
                .serve(sacp_tokio::Stdio::new())
                .await?;
        }

        Command::Eliza => {
            // Run the built-in Eliza agent directly (no Symposium wrapping)
            elizacp::ElizaAgent::new(false)
                .serve(sacp_tokio::Stdio::new())
                .await?;
        }

        Command::Vscodelm { trace_dir } => {
            // Run as VS Code Language Model Provider backend
            vscodelm::serve_stdio(trace_dir).await?;
        }
    }

    Ok(())
}
