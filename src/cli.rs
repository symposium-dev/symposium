//! CLI argument parsing and dispatch.
//!
//! This module defines the argument types and the core `run()` function
//! so that both the binary and the test harness can invoke commands.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::config::Symposium;
use crate::crate_command::{self, DispatchResult};
use crate::hook;
use crate::init::{self, InitOpts};
use crate::install;
use crate::output::Output;
use crate::plugins::Audience;
use crate::self_update;
use crate::status;
use crate::subcommand_dispatch::dispatch_external;
use crate::sync;

/// Parsed CLI arguments.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-agents",
    bin_name = "cargo agents",
    version,
    about = "AI the Rust Way",
    allow_external_subcommands = true,
    disable_help_flag = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Suppress status output
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Print detailed information about decisions made
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output structured JSON report
    #[arg(long, global = true)]
    pub json: bool,

    /// Print help
    #[arg(short = 'h', long = "help", global = true)]
    pub help: bool,

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

    /// Show resolved plugin/skill state for the current workspace
    Status,

    /// Install plugin sources into user config
    Install {
        /// Crate source to install, optionally as <CRATE>@<VERSION>. Repeatable.
        crates: Vec<String>,

        /// Direct path plugin source to install. Repeatable.
        #[arg(long = "path")]
        paths: Vec<String>,

        /// Direct git plugin source to install. Repeatable.
        #[arg(long = "git")]
        git: Vec<String>,
    },

    /// Uninstall plugin sources from user config
    Uninstall {
        /// Crate source name to uninstall. Repeatable.
        crates: Vec<String>,

        /// Direct path plugin source to uninstall. Repeatable.
        #[arg(long = "path")]
        paths: Vec<String>,

        /// Direct git plugin source to uninstall. Repeatable.
        #[arg(long = "git")]
        git: Vec<String>,
    },

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
    CrateInfo {
        /// Crate name
        name: String,

        /// Version constraint (e.g., "1.0.3", "^1.0"); defaults to workspace version or latest
        #[arg(long)]
        version: Option<String>,
    },

    /// Plugin-vended subcommand. `argv[0]` is the name; the rest forwards to the child.
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

/// Audience section a built-in subcommand belongs to in `--help`.
///
/// `None` means the subcommand is hidden (omitted from help entirely).
/// Plugin-vended subcommands carry their own audience on the manifest;
/// this only covers the static `Commands` variants above.
pub fn builtin_audience(name: &str) -> Option<Audience> {
    match name {
        "init" | "install" | "plugin" | "self-update" | "status" | "sync" | "uninstall" => {
            Some(Audience::Humans)
        }
        "crate-info" => Some(Audience::Agents),
        _ => None,
    }
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
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

        Commands::Sync => sync::sync(sym, &mut sym.workspace_deps(cwd)).await,

        Commands::Status => status::status(sym, &mut sym.workspace_deps(cwd)).await,

        Commands::Install { crates, paths, git } => install::install(sym, crates, paths, git, out),

        Commands::Uninstall { crates, paths, git } => {
            install::uninstall(sym, crates, paths, git, out)
        }

        Commands::SelfUpdate => self_update::self_update(sym, out),

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

        Commands::External(argv) => {
            let result = dispatch_external(sym, cwd, argv).await?;
            if !result.stdout.is_empty() {
                out.println(String::from_utf8_lossy(&result.stdout).trim_end());
            }
            if !result.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&result.stderr));
            }
            match result.exit_code {
                0 => Ok(()),
                code => bail!("subcommand exited with status: {code}"),
            }
        }
        // These commands can't easily be extracted since they do I/O
        // (stdin/stdout for hooks). The binary handles them directly.
        Commands::Hook { .. } | Commands::Plugin { .. } => {
            anyhow::bail!("command not supported in library dispatch (use binary)")
        }
    }
}
