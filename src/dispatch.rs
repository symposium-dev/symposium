//! Shared dispatch logic for CLI and MCP.
//!
//! Both CLI subcommands and the MCP `rust` tool route through `dispatch()`.

use std::path::Path;

use crate::config::Symposium;
use crate::crate_sources;
use crate::plugins;
use crate::skills;

/// Result of dispatching a command.
pub enum DispatchResult {
    /// Successful output to display.
    Ok(String),
    /// Error message.
    Err(String),
}

/// Dispatch a command given as a list of string arguments.
///
/// This is the shared entry point for both CLI and MCP.
///
/// Supported forms:
/// - `["start"]` — Rust guidance + dynamic crate skill list
/// - `["crate", "--list"]` — list workspace crates with available skills
/// - `["crate", "<name>"]` — crate info + guidance
/// - `["crate", "<name>", "--version", "<ver>"]` — crate info with version
/// - `["help"]` or `[]` — help text
pub async fn dispatch(sym: &Symposium, args: &[String], cwd: &Path) -> DispatchResult {
    if args.is_empty() || (args.len() == 1 && args[0] == "help") {
        return DispatchResult::Ok(help_text());
    }

    match args[0].as_str() {
        "start" => dispatch_start(sym, cwd).await,
        "crate" => dispatch_crate(sym, &args[1..], cwd).await,
        other => DispatchResult::Err(format!(
            "Unknown command: {other}. Use `help` to see available commands."
        )),
    }
}

async fn dispatch_start(sym: &Symposium, cwd: &Path) -> DispatchResult {
    let tutorial = crate::tutorial::render_cli();

    let workspace = crate_sources::workspace_semver_pairs(cwd);
    let registry = plugins::load_registry(sym);
    let skill_list = skills::list_output(sym, &registry, &workspace).await;

    let mut output = tutorial;
    output.push_str("\n\n");
    output.push_str(&skill_list);

    DispatchResult::Ok(output)
}

async fn dispatch_crate(sym: &Symposium, args: &[String], cwd: &Path) -> DispatchResult {
    // Parse crate subcommand arguments
    let mut name: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut list = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--list" => list = true,
            "--version" => {
                i += 1;
                if i < args.len() {
                    version = Some(&args[i]);
                } else {
                    return DispatchResult::Err("--version requires a value".to_string());
                }
            }
            arg if !arg.starts_with('-') => {
                name = Some(arg);
            }
            other => {
                return DispatchResult::Err(format!("Unknown option: {other}"));
            }
        }
        i += 1;
    }

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

fn help_text() -> String {
    "\
Symposium — AI the Rust Way

Commands:
  start                    Get Rust guidance and list available crate skills
  crate --list             List workspace crates with available skills
  crate <name>             Get crate info and guidance
  crate <name> --version <ver>  Get crate info for a specific version
  help                     Show this message"
        .to_string()
}
