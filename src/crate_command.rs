//! Crate command: fetch crate sources by name and version.

use std::path::Path;

use crate::config::Symposium;
use crate::crate_sources::normalize_crate_name;
use crate::pm::CargoPm;

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
    tracing::debug!(%name, ?version, "crate-info dispatched");
    let deps = sym.workspace_deps(cwd);
    if version.is_none() {
        let normalized = normalize_crate_name(name);
        if let Some(member) = deps
            .member_crates()
            .iter()
            .find(|m| normalize_crate_name(&m.name) == normalized)
            && let Some(dir) = &member.path
        {
            return crate_info(&member.name, &member.version.to_string(), dir);
        }
    }
    let id = CargoPm::id_for(name, version);
    match sym
        .package_managers(&deps)
        .fetch(&id, symposium_install::UpdateLevel::Check)
        .await
    {
        Ok(result) => crate_info(
            &result.package.id.name,
            &result.package.id.version,
            &result.root,
        ),
        Err(e) => DispatchResult::Err(format!("{e}")),
    }
}

fn crate_info(name: &str, version: &str, source: &Path) -> DispatchResult {
    let output = format!(
        "Crate: {name}\nVersion: {version}\nSource: {}\n",
        source.display()
    );
    tracing::trace!(%output, "crate-info output");
    DispatchResult::Ok(output)
}
