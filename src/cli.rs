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
use crate::self_update;
use crate::sync;

/// Parsed CLI arguments.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-agents",
    bin_name = "cargo agents",
    version,
    about = "AI the Rust Way"
)]
pub struct Cli {
    /// Control plugin source update behavior (none, check, fetch)
    #[arg(long, global = true, default_value = "none")]
    pub update: crate::plugins::UpdateLevel,

    /// Suppress status output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Set up user-wide configuration
    Init {
        /// Agent to configure (e.g., claude, copilot, gemini). Repeatable.
        /// Skips the interactive prompt.
        #[arg(long = "add-agent")]
        agents: Vec<String>,

        /// Remove an agent. Repeatable.
        #[arg(long = "remove-agent")]
        remove_agents: Vec<String>,

        /// Where to install agent hooks: global (~/) or project (./).
        #[arg(long = "hook-scope")]
        hook_scope: Option<crate::config::HookScope>,
    },

    /// Synchronize skills with workspace dependencies
    Sync,

    /// Hook entry point invoked by your agent (internal)
    #[command(hide = true)]
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

    /// Update symposium to the latest version
    SelfUpdate,

    /// Find crate sources
    #[command(hide = true)]
    CrateInfo {
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
    // Periodic update check (skipped for self-update, which always checks).
    // In the binary, re-exec happens if auto-update = "on" succeeds.
    // Here in the library we just run the warn check; the binary wraps
    // this with re-exec logic.
    if !matches!(cmd, Commands::SelfUpdate) {
        self_update::maybe_warn_for_update(sym, out);
    }

    match cmd {
        Commands::Init {
            agents,
            remove_agents,
            hook_scope,
        } => {
            let opts = InitOpts {
                agents,
                remove_agents,
                hook_scope,
            };
            init::init(sym, out, &opts).await
        }

        Commands::Sync => sync::sync(sym, cwd, out).await,

        Commands::SelfUpdate => self_update::self_update(sym, out).await,

        Commands::CrateInfo { name, version } => {
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
