//! Crate command: fetch crate sources by name and version.

use std::path::Path;

use crate::config::Symposium;
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
    let mut deps = sym.workspace_deps(cwd);
    let workspace = deps.crates();
    let id = CargoPm::id_for(name, version);
    let cx = crate::pm::PmContext::new(sym, workspace);
    match sym
        .package_managers()
        .fetch(&id, &cx, symposium_install::UpdateLevel::None)
        .await
    {
        Ok(result) => {
            let output = format!(
                "Crate: {}\nVersion: {}\nSource: {}\n",
                result.id.name,
                result.id.version,
                result.root.display()
            );
            tracing::trace!(%output, "crate-info output");
            DispatchResult::Ok(output)
        }
        Err(e) => DispatchResult::Err(format!("{e}")),
    }
}
