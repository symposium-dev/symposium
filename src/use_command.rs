//! `cargo agents use` — explicit plugin enablement.
//!
//! Enablement is the consent axis: the workspace and the configured
//! registries are trust roots, but a dependency is not, so a plugin embedded
//! in a dependency runs only once the user says so. `use` is the durable,
//! by-name form of that decision — it writes a [`UseEntry`] into `[plugins]
//! use`, scoped to the current workspace by default or to every workspace
//! with `--global`.
//!
//! It is also what wakes a *dormant* registry plugin (one whose manifest
//! names no dependency, so nothing else would ever gate it on —
//! [`Plugin::requires_use`](crate::plugins::Plugin::requires_use)).
//!
//! `use` only adds to what *may* run; activation predicates still decide
//! when it applies. `--remove` is the inverse, and re-syncs so the plugin's
//! skills are reaped straight away.

use std::path::Path;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::config::{Symposium, UseEntry};
use crate::crate_sources::normalize_crate_name;
use crate::report::ReportEvent;

/// Record an enablement for `name` and sync so its skills install now.
pub async fn use_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    // A configured registry is a trust root: what it offers is already
    // enabled by configuration, so there is nothing to record. The exception
    // is a dormant plugin, for which `use` is exactly the wake-up call.
    let registry = crate::plugins::load_registry(sym).await;
    let normalized = normalize_crate_name(name);
    let registry_plugin = registry
        .plugins
        .iter()
        .find(|p| normalize_crate_name(&p.plugin.name) == normalized);
    let dormant = registry_plugin.is_some_and(|p| p.plugin.requires_use);
    let already_trusted = (registry_plugin.is_some() && !dormant)
        || registry
            .standalone_skills
            .iter()
            .any(|s| s.skill.name() == name);
    if already_trusted {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "`{name}` is already available from a configured registry; nothing to enable"
                ),
            },
        );
        return Ok(());
    }

    let mut deps = sym.workspace_deps(cwd);
    let workspace_root = deps.load().map(|ws| ws.root.clone());
    if !global && workspace_root.is_none() {
        bail!("not in a Rust workspace; pass --global to enable `{name}` everywhere");
    }

    if !dormant {
        resolve_name(sym, &mut deps, name).await?;
    }

    let entry = match &workspace_root {
        _ if global => UseEntry::Global(name.to_string()),
        Some(root) => UseEntry::Workspace {
            name: name.to_string(),
            workspace: root.clone(),
        },
        None => unreachable!("checked above"),
    };

    if sym.config.plugins.used.contains(&entry) {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("`{name}` is already enabled; nothing changed"),
            },
        );
    } else {
        sym.config.plugins.used.push(entry);
        sym.save_config().context("failed to write user config")?;
        tracing::info!(
            report = %ReportEvent::PluginEnabled {
                name: name.to_string(),
                global,
            },
        );
    }

    // Install now rather than waiting for the next sync.
    if workspace_root.is_some() {
        crate::sync::sync(sym, &mut deps, update).await?;
    }
    Ok(())
}

/// Drop a previously recorded enablement and re-sync so the plugin's skills
/// are reaped now.
///
/// The scope must match: without `--global` this removes the entry recorded
/// for the current workspace, with it the unscoped entry. A scope mismatch is
/// an error rather than a silent success.
pub async fn remove_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let mut deps = sym.workspace_deps(cwd);
    let workspace_root = deps.load().map(|ws| ws.root.clone());

    let used = &mut sym.config.plugins.used;
    let before = used.len();
    used.retain(|entry| {
        if normalize_crate_name(entry.name()) != normalize_crate_name(name) {
            return true;
        }
        let in_scope = match entry {
            UseEntry::Global(_) => global,
            UseEntry::Workspace { .. } => {
                !global
                    && workspace_root
                        .as_deref()
                        .is_some_and(|root| entry.applies_in(root))
            }
        };
        !in_scope
    });
    if used.len() == before {
        let scope = if global { "--global" } else { "this workspace" };
        bail!("no `use` entry for `{name}` ({scope}); see `cargo agents status`");
    }
    sym.save_config().context("failed to write user config")?;
    tracing::info!(
        report = %ReportEvent::PluginRemoved {
            name: name.to_string(),
            global,
        },
    );

    if workspace_root.is_some() {
        crate::sync::sync(sym, &mut deps, update).await?;
    }
    Ok(())
}

/// Check that `name` resolves to something before recording it: a workspace
/// dependency (offline-friendly) or a registry search hit.
async fn resolve_name(
    sym: &Symposium,
    deps: &mut symposium_sdk::workspace::WorkspaceDeps,
    name: &str,
) -> Result<()> {
    let normalized = normalize_crate_name(name);
    let is_workspace_dep = deps.load().is_some_and(|ws| {
        ws.crates
            .iter()
            .any(|c| normalize_crate_name(&c.name) == normalized)
    });
    if is_workspace_dep {
        return Ok(());
    }

    let cx = crate::pm::PmContext::new(sym, deps.crates());
    let found = sym
        .package_managers()
        .search(name, &cx)
        .await
        .iter()
        .any(|(_, info)| normalize_crate_name(&info.id.name) == normalized);
    if found {
        return Ok(());
    }
    bail!("no crate or plugin named `{name}` found (try `cargo agents search {name}`)")
}
