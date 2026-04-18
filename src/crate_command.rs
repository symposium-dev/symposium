//! Crate command: fetch crate sources by name and version.

use std::path::Path;

use crate::config::Symposium;
use crate::crate_sources;

/// Result of dispatching a command.
pub enum DispatchResult {
    /// Successful output to display.
    Ok(String),
    /// Error message.
    Err(String),
}

pub async fn dispatch_crate(
    sym: &Symposium,
    name: &str,
    version: Option<&str>,
    cwd: &Path,
) -> DispatchResult {
    let workspace = crate_sources::workspace_semver_pairs(cwd);
    let mut fetch = crate_sources::RustCrateFetch::new(name, &workspace, sym.cache_dir());
    if let Some(v) = version {
        fetch = fetch.version(v);
    }

    match fetch.fetch().await {
        Ok(result) => {
            let output = format!(
                "Crate: {}\nVersion: {}\nSource: {}\n",
                result.name,
                result.version,
                result.path.display()
            );
            DispatchResult::Ok(output)
        }
        Err(e) => DispatchResult::Err(format!("{e}")),
    }
}
