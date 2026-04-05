//! Shared dispatch logic for CLI and MCP.
//!
//! Defines `SharedCommand` (the subset of commands common to CLI and MCP)
//! with Clap derive. The CLI flattens these into its own command enum.
//! The MCP parses incoming args via `McpArgs::try_parse_from` (in `mcp.rs`).

use std::path::Path;

use clap::Subcommand;

use crate::config::Symposium;
use crate::crate_sources;
use crate::plugins;
use crate::skills;

/// Commands shared between CLI and MCP.
#[derive(Debug, Clone, Subcommand)]
pub enum SharedCommand {
    /// Get Rust guidance and list available crate skills for the workspace
    Start,

    /// Find crate sources and guidance
    Crate {
        /// Crate name (omit to use --list)
        name: Option<String>,

        /// Version constraint (e.g., "1.0.3", "^1.0"); defaults to workspace version or latest
        #[arg(long)]
        version: Option<String>,

        /// List all workspace dependency crates
        #[arg(long)]
        list: bool,
    },
}

/// Whether the caller is the CLI or the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Cli,
    Mcp,
}

/// Result of dispatching a command.
pub enum DispatchResult {
    /// Successful output to display.
    Ok(String),
    /// Error message.
    Err(String),
}

/// Dispatch a shared command.
pub async fn dispatch(
    sym: &Symposium,
    cmd: SharedCommand,
    cwd: &Path,
    mode: RenderMode,
) -> DispatchResult {
    match cmd {
        SharedCommand::Start => dispatch_start(sym, cwd, mode).await,
        SharedCommand::Crate {
            name,
            version,
            list,
        } => dispatch_crate(sym, name.as_deref(), version.as_deref(), list, cwd).await,
    }
}

async fn dispatch_start(sym: &Symposium, cwd: &Path, mode: RenderMode) -> DispatchResult {
    let tutorial = match mode {
        RenderMode::Cli => crate::tutorial::render_cli(),
        RenderMode::Mcp => crate::tutorial::render_mcp(),
    };

    let workspace = crate_sources::workspace_semver_pairs(cwd);
    let registry = plugins::load_registry(sym);
    let skill_list = skills::list_output(sym, &registry, &workspace).await;

    let mut output = tutorial;
    output.push_str("\n\n");
    output.push_str(&skill_list);

    DispatchResult::Ok(output)
}

async fn dispatch_crate(
    sym: &Symposium,
    name: Option<&str>,
    version: Option<&str>,
    list: bool,
    cwd: &Path,
) -> DispatchResult {
    let workspace = crate_sources::workspace_semver_pairs(cwd);
    let registry = plugins::load_registry(sym);

    if list {
        let output = skills::list_output(sym, &registry, &workspace).await;
        DispatchResult::Ok(output)
    } else if let Some(name) = name {
        match skills::info_output(sym, name, version, &registry, &workspace).await {
            Ok(output) => DispatchResult::Ok(output),
            Err(e) => DispatchResult::Err(format!("{e}")),
        }
    } else {
        DispatchResult::Err("Provide a crate name or use --list".to_string())
    }
}

