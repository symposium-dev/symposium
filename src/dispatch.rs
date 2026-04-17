//! Shared dispatch logic for CLI and MCP.

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

pub async fn dispatch_crate(
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
