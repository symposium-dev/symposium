//! Symposium ACP Agent
//!
//! A unified binary for running Symposium-enriched ACP agents and proxies.
//!
//! ## Commands
//!
//! ### run-with
//! Run Symposium with a specific agent configuration:
//! ```bash
//! # Agent mode: wrap a downstream agent
//! symposium-acp-agent run-with --proxy sparkle --proxy ferris --agent '{"name":"...","command":"npx",...}'
//!
//! # Proxy mode: sit between editor and existing agent
//! symposium-acp-agent run-with --proxy sparkle --proxy ferris
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
//! Use `--proxy <json>` to specify extensions. Order matters - proxies are
//! chained in the order specified. Use `registry resolve-extension` to get json.

use anyhow::Result;
use clap::{Parser, Subcommand};
use sacp::{Component, DynComponent, ProxyToConductor};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use std::str::FromStr;

mod config;

use symposium_acp_agent::registry::{self, built_in_proxies};
use symposium_acp_agent::symposium::{Symposium, SymposiumConfig};
use symposium_acp_agent::vscodelm;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-agent")]
#[command(about = "Symposium-enriched ACP agent and proxy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run Symposium with a specific agent configuration
    ///
    /// Without --agent: proxy mode (sits between editor and existing agent)
    /// With --agent: agent mode (wraps the specified downstream agent)
    RunWith {
        /// Extension proxy to include in the chain (can be specified multiple times).
        /// Order matters - proxies are chained in the order specified.
        ///
        /// JSON from `registry resolve-extension` is expected.
        #[arg(long = "proxy", value_name = "NAME")]
        proxies: Vec<String>,

        /// Agent specification: JSON from `registry resolve-agent` or a command string.
        /// If omitted, runs in proxy mode.
        #[arg(long)]
        agent: Option<String>,

        /// Enable trace logging to the specified directory.
        /// Traces are written as timestamped .jsons files viewable with sacp-trace-viewer.
        #[arg(long)]
        trace_dir: Option<PathBuf>,

        /// Enable logging to stderr. Accepts a level (error, warn, info, debug, trace)
        /// or a RUST_LOG-style filter string (e.g., "sacp=debug,symposium=trace").
        #[arg(long)]
        log: Option<String>,
    },

    /// Run the built-in Eliza agent (useful for testing)
    Eliza,

    /// Run as a VS Code Language Model Provider backend
    Vscodelm {
        /// Enable trace logging to the specified directory
        #[arg(long)]
        trace_dir: Option<PathBuf>,
    },

    /// Run using configuration from ~/.symposium/config.jsonc
    ///
    /// If no configuration file exists, runs an interactive setup wizard
    /// to help create one.
    Run {
        /// Enable trace logging to the specified directory
        #[arg(long)]
        trace_dir: Option<PathBuf>,

        /// Enable logging to stderr. Accepts a level (error, warn, info, debug, trace)
        /// or a RUST_LOG-style filter string.
        #[arg(long)]
        log: Option<String>,
    },

    /// Agent registry commands (for tooling integration)
    #[command(subcommand)]
    Registry(RegistryCommand),

    /// Act as a shim around a builtin proxy (i.e. ferris and cargo).
    /// Will be removed when those are published and can be used "normally".
    ProxyShim {
        #[arg(long)]
        proxy: String,
    },
}

/// Registry subcommands - output JSON for tooling integration
#[derive(Subcommand, Debug)]
enum RegistryCommand {
    /// List all available agents (built-ins + registry)
    List,

    /// List all available extensions from the registry
    ListExtensions,

    /// Resolve an agent ID to an executable McpServer configuration.
    /// Downloads binaries if needed.
    ResolveAgent {
        /// The agent ID to resolve
        agent_id: String,
    },

    ResolveExtension {
        /// The extension to resolve
        extension_id: String,
    },
}

/// Build proxy components from the configured sources, preserving order.
fn build_proxies(raw_proxies: Vec<String>) -> Result<Vec<DynComponent<ProxyToConductor>>> {
    let mut proxies = Vec::with_capacity(raw_proxies.len());
    for proxy in raw_proxies {
        proxies.push(DynComponent::new(AcpAgent::from_str(&proxy)?));
    }

    Ok(proxies)
}

/// Set up logging if requested.
fn setup_logging(log: Option<String>) {
    if let Some(filter) = &log {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(filter))
            .with_writer(std::io::stderr)
            .init();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::RunWith {
            proxies,
            agent,
            trace_dir,
            log,
        } => {
            let proxies = build_proxies(proxies)?;
            let mut config = SymposiumConfig::new();

            if let Some(trace_dir) = trace_dir {
                config = config.trace_dir(trace_dir);
            }

            setup_logging(log);

            let symposium = Symposium::new(config, proxies);
            if let Some(agent_spec) = agent {
                let agent: AcpAgent = agent_spec.parse()?;
                tracing::debug!(
                    "Starting in agent mode with downstream: {:?}",
                    agent.server()
                );
                symposium
                    .with_agent(agent)
                    .serve(sacp_tokio::Stdio::new())
                    .await?;
            } else {
                tracing::debug!("Starting in proxy mode");
                symposium.serve(sacp_tokio::Stdio::new()).await?;
            }
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

        Command::Run { trace_dir, log } => {
            // Set up logging if requested
            if let Some(filter) = &log {
                use tracing_subscriber::EnvFilter;
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::new(filter))
                    .with_writer(std::io::stderr)
                    .init();
            }

            match config::SymposiumUserConfig::load()? {
                Some(user_config) => {
                    // Run with the loaded configuration
                    let proxy_names = user_config.enabled_proxies();
                    let agent_args = user_config.agent_args()?;

                    // The user config is currently just a set of (builtin) proxy names.
                    let mut config = SymposiumConfig::new();
                    if let Some(trace_dir) = trace_dir {
                        config = config.trace_dir(trace_dir);
                    }

                    let possible_proxies = built_in_proxies()?;
                    let mut proxies = vec![];
                    for name in proxy_names {
                        if let Some(entry) = possible_proxies.iter().find(|p| p.id == name) {
                            let server = crate::registry::resolve_distribution(entry)
                                .await
                                .map_err(|e| sacp::Error::new(-32603, e.to_string()))?;
                            let server = server
                                .ok_or_else(|| sacp::Error::new(-32603, "Extension not found."))?;
                            proxies.push(DynComponent::new(AcpAgent::new(server)));
                        }
                    }
                    let agent = AcpAgent::from_args(&agent_args)?;

                    tracing::debug!(
                        "Starting in configured mode with agent: {:?}",
                        agent.server()
                    );

                    Symposium::new(config, proxies)
                        .with_agent(agent)
                        .serve(sacp_tokio::Stdio::new())
                        .await?;
                }
                None => {
                    // No config - run configuration agent
                    config::ConfigurationAgent::new()
                        .await
                        .serve(sacp_tokio::Stdio::new())
                        .await?;
                }
            }
        }

        Command::Registry(registry_cmd) => match registry_cmd {
            RegistryCommand::List => {
                let agents = registry::list_agents().await?;
                println!("{}", serde_json::to_string(&agents)?);
            }
            RegistryCommand::ListExtensions => {
                let extensions = registry::list_extensions().await?;
                println!("{}", serde_json::to_string(&extensions)?);
            }
            RegistryCommand::ResolveAgent { agent_id: agent } => {
                let server = registry::resolve_agent(&agent).await?;
                println!("{}", serde_json::to_string(&server)?);
            }
            RegistryCommand::ResolveExtension { extension_id } => {
                let server = registry::resolve_extension(&extension_id).await?;
                println!("{}", serde_json::to_string(&server)?);
            }
        },

        Command::ProxyShim { proxy } => match proxy.as_str() {
            "ferris" => {
                symposium_ferris::FerrisComponent::default()
                    .serve(sacp_tokio::Stdio::new())
                    .await?;
            }
            "cargo" => {
                symposium_cargo::CargoProxy
                    .serve(sacp_tokio::Stdio::new())
                    .await?;
            }
            _ => {
                anyhow::bail!("Unexpected proxy {proxy}. Expected one of `ferris` or `cargo`.");
            }
        },
    }

    Ok(())
}
