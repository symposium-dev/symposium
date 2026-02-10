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
//! Use `--proxy <json>` to specify mods. Order matters - proxies are
//! chained in the order specified. Use `registry resolve-mod` to get json.

use anyhow::Result;
use clap::{Parser, Subcommand};
use sacp::{Component, DynComponent, ProxyToConductor};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use std::str::FromStr;

use symposium_acp_agent::ConfigAgent;
use symposium_acp_agent::recommendations::RecommendationsExt;
use symposium_acp_agent::registry;
use symposium_acp_agent::remote_recommendations;
use symposium_acp_agent::symposium::{Symposium, SymposiumConfig};
use symposium_acp_agent::user_config::{ConfigPaths, GlobalAgentConfig, WorkspaceModsConfig};
use symposium_acp_agent::vscodelm;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-agent")]
#[command(about = "Symposium-enriched ACP agent and proxy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Shared logging and tracing options
#[derive(clap::Args, Debug, Clone)]
struct LoggingOptions {
    /// Enable trace logging to the specified directory.
    /// Traces are written as timestamped .jsons files viewable with sacp-trace-viewer.
    #[arg(long)]
    trace_dir: Option<PathBuf>,

    /// Enable logging to stderr. Accepts a level (error, warn, info, debug, trace)
    /// or a RUST_LOG-style filter string (e.g., "sacp=debug,symposium=trace").
    #[arg(long)]
    log: Option<String>,

    /// Write logs to a timestamped file in the specified directory instead of stderr.
    #[arg(long)]
    log_dir: Option<PathBuf>,
}

impl LoggingOptions {
    /// Set up logging based on the options.
    fn setup(&self) {
        setup_logging(self.log.clone(), self.log_dir.clone());
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run Symposium with a specific agent configuration
    ///
    /// Without --agent: proxy mode (sits between editor and existing agent)
    /// With --agent: agent mode (wraps the specified downstream agent)
    RunWith {
        /// Mod proxy to include in the chain (can be specified multiple times).
        /// Order matters - proxies are chained in the order specified.
        ///
        /// JSON from `registry resolve-mod` is expected.
        #[arg(long = "proxy", value_name = "NAME")]
        proxies: Vec<String>,

        /// Agent specification: JSON from `registry resolve-agent` or a command string.
        /// If omitted, runs in proxy mode.
        #[arg(long)]
        agent: Option<String>,

        #[command(flatten)]
        logging: LoggingOptions,
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
        #[command(flatten)]
        logging: LoggingOptions,
    },

    /// Agent registry commands (for tooling integration)
    #[command(subcommand)]
    Registry(RegistryCommand),

    /// Initialize workspace configuration (useful for CI/testing)
    ///
    /// Creates both workspace-specific config and global agent default.
    Init {
        /// Workspace directory to initialize
        workspace: PathBuf,

        /// Agent ID (e.g., "elizacp", "zed-claude-code") - same format as `registry resolve-agent`
        #[arg(long, default_value = "elizacp")]
        agent: String,

        /// Skip mod recommendations (create config with no mods)
        #[arg(long)]
        no_mods: bool,
    },
}

/// Registry subcommands - output JSON for tooling integration
#[derive(Subcommand, Debug)]
enum RegistryCommand {
    /// List all available agents (built-ins + registry)
    List,

    /// List all available mods from the registry
    ListMods,

    /// Resolve an agent ID to an executable McpServer configuration.
    /// Downloads binaries if needed.
    ResolveAgent {
        /// The agent ID to resolve
        agent_id: String,
    },

    ResolveMod {
        /// The mod to resolve
        mod_id: String,
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
fn setup_logging(log: Option<String>, log_dir: Option<PathBuf>) {
    if let Some(filter) = &log {
        use tracing_subscriber::EnvFilter;

        if let Some(dir) = log_dir {
            // Create timestamped log file in the specified directory
            let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let log_path = dir.join(format!("{}.log", timestamp));

            // Ensure directory exists
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("Failed to create log directory {:?}: {}", dir, e);
                return;
            }

            let file = match std::fs::File::create(&log_path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to create log file {:?}: {}", log_path, e);
                    return;
                }
            };

            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(filter))
                .with_writer(file)
                .with_ansi(false)
                .init();
        } else {
            // Write to stderr
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(filter))
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::RunWith {
            proxies,
            agent,
            logging,
        } => {
            logging.setup();

            let proxies = build_proxies(proxies)?;
            let mut config = SymposiumConfig::new();

            if let Some(trace_dir) = logging.trace_dir {
                config = config.trace_dir(trace_dir);
            }

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

        Command::Run { logging } => {
            logging.setup();

            // ConfigAgent handles both configured and unconfigured states:
            // - If config exists: creates conductors and delegates sessions
            // - If no config: runs initial setup wizard
            // - Handles /symposium:config command for runtime configuration
            //
            // ConfigAgent::new() loads recommendations from remote (with cache fallback).
            // If recommendations can't be loaded at all, we fail here with a clear error.
            let mut agent = ConfigAgent::new().await?;
            if let Some(dir) = logging.trace_dir {
                agent = agent.with_trace_dir(dir);
            }
            agent.serve(sacp_tokio::Stdio::new()).await?;
        }

        Command::Registry(registry_cmd) => match registry_cmd {
            RegistryCommand::List => {
                let agents = registry::list_agents().await?;
                println!("{}", serde_json::to_string(&agents)?);
            }
            RegistryCommand::ListMods => {
                let mods = registry::list_mods().await?;
                println!("{}", serde_json::to_string(&mods)?);
            }
            RegistryCommand::ResolveAgent { agent_id: agent } => {
                let server = registry::resolve_agent(&agent).await?;
                println!("{}", serde_json::to_string(&server)?);
            }
            RegistryCommand::ResolveMod { mod_id } => {
                let server = registry::resolve_mod(&mod_id).await?;
                println!("{}", serde_json::to_string(&server)?);
            }
        },

        Command::Init {
            workspace,
            agent,
            no_mods,
        } => {
            let config_paths = ConfigPaths::default_location()?;
            let agent = registry::lookup_agent_source(&agent).await?;

            let recs = if no_mods {
                vec![]
            } else {
                // Load recommendations from remote (with cache fallback)
                let recommendations =
                    remote_recommendations::load_recommendations(&config_paths).await?;
                recommendations.for_workspace(&workspace).mods
            };

            // Save workspace mods
            let mods_config = WorkspaceModsConfig::from_recommendations(recs);
            mods_config.save(&config_paths, &workspace).await?;

            // Save global agent
            let global_config = GlobalAgentConfig::new(agent);
            global_config.save(&config_paths).await?;

            eprintln!("Initialized config for {}", workspace.display());
        }
    }

    Ok(())
}
