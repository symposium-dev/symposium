//! CLI argument parsing and dispatch.
//!
//! This module defines the argument types and the core `run()` function
//! so that both the binary and the test harness can invoke commands.

use std::path::Path;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Symposium;
use crate::crate_command::{self, DispatchResult};
use crate::hook;
use crate::init::{self, InitOpts};
use crate::output::Output;
use crate::sync;

/// Parsed CLI arguments.
#[derive(Debug, Parser)]
#[command(
    name = "symposium",
    bin_name = "symposium",
    version,
    about = "AI the Rust Way"
)]
pub struct Cli {
    /// Control plugin source update behavior (none, check, fetch)
    #[arg(long, global = true, default_value = "none")]
    pub update: crate::git_source::UpdateLevel,

    /// Suppress status output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Set up user-wide or project-level configuration
    Init {
        /// Set up user-wide configuration only
        #[arg(long)]
        user: bool,

        /// Set up project configuration only
        #[arg(long)]
        project: bool,

        /// Agent to configure (e.g., claude, copilot, gemini). Repeatable.
        /// Skips the interactive prompt.
        #[arg(long = "add-agent")]
        agents: Vec<String>,

        /// Remove an agent. Repeatable.
        #[arg(long = "remove-agent")]
        remove_agents: Vec<String>,
    },

    /// Synchronize configuration with workspace dependencies and agent
    Sync {
        /// Only update .symposium/config.toml from workspace dependencies
        #[arg(long)]
        workspace: bool,

        /// Only install enabled extensions into the agent's directories
        #[arg(long)]
        agent: bool,

        /// Add an agent to the project config. Repeatable.
        #[arg(long = "add-agent", value_name = "NAME")]
        add_agents: Vec<String>,

        /// Remove an agent from the project config. Repeatable.
        #[arg(long = "remove-agent", value_name = "NAME")]
        remove_agents: Vec<String>,
    },

    /// Hook entry point invoked by your agent (internal)
    Hook {
        /// The agent (claude, copilot, gemini)
        agent: hook::HookAgent,

        /// The hook event (e.g., pre-tool-use, post-tool-use)
        event: hook::HookEvent,
    },

    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },

    /// Find crate sources
    Crate {
        /// Crate name
        name: String,

        /// Version constraint (e.g., "1.0.3", "^1.0"); defaults to workspace version or latest
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Sync plugin sources from git repositories
    Sync {
        /// Provider name to sync (omit to sync all)
        provider: Option<String>,
    },

    /// List all providers and their plugins
    List,

    /// Show details for a specific plugin
    Show {
        /// Plugin name
        plugin: String,
    },

    /// Validate a plugin source directory or a single TOML manifest
    Validate {
        /// Path to a directory or a single .toml file
        path: std::path::PathBuf,

        /// Skip checking that crate names in predicates exist on crates.io
        #[arg(long)]
        no_check_crates: bool,
    },
}

/// Run a parsed CLI command.
///
/// `cwd` is the working directory for commands that need it (sync, start, crate).
/// The binary passes `std::env::current_dir()`; tests pass the fixture workspace root.
pub async fn run(sym: &mut Symposium, cmd: Commands, cwd: &Path, out: &Output) -> Result<()> {
    match cmd {
        Commands::Init {
            user,
            project,
            agents,
            remove_agents,
        } => {
            let opts = InitOpts {
                agents,
                remove_agents,
            };
            if user && !project {
                init::init_user(sym, out, &opts).await
            } else if project && !user {
                init::init_project(sym, cwd, out, &opts).await
            } else {
                init::init_default(sym, cwd, out, &opts).await
            }
        }

        Commands::Sync {
            workspace,
            agent,
            add_agents,
            remove_agents,
        } => {
            for name in &add_agents {
                sync::add_agent(cwd, name, out)?;
            }
            for name in &remove_agents {
                sync::remove_agent(cwd, name, out)?;
            }

            let do_workspace = workspace || (!workspace && !agent);
            let do_agent = agent || (!workspace && !agent);

            if do_workspace {
                sync::sync_workspace(sym, cwd, out).await?;
            }

            if do_agent {
                sync::sync_agent(sym, Some(cwd), out).await?;
            }

            Ok(())
        }

        Commands::Crate { name, version } => {
            match crate_command::dispatch_crate(sym, &name, version.as_deref(), cwd).await {
                DispatchResult::Ok(output) => {
                    print!("{output}");
                    Ok(())
                }
                DispatchResult::Err(e) => {
                    anyhow::bail!("{e}");
                }
            }
        }

        // These commands can't easily be extracted since they do I/O
        // (stdin/stdout for hooks). The binary handles them directly.
        Commands::Hook { .. } | Commands::Plugin { .. } => {
            anyhow::bail!("command not supported in library dispatch (use binary)")
        }
    }
}
