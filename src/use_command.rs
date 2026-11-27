//! `symposium use`: explicit plugin enablement.
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
//! [`PluginManifest::requires_use`](crate::plugins::PluginManifest::requires_use)).
//!
//! `use` only adds to what *may* run; activation predicates still decide
//! when it applies. `--remove` is the inverse, and re-syncs so the plugin's
//! skills are reaped straight away.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::config::{Symposium, UseEntry};
use crate::plugins::Plugin;
use crate::pm::{PackageId, PmRegistry, Workspace};
use crate::report::ReportEvent;

/// Record an enablement for `name` and sync so its skills install now.
pub async fn use_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let deps = sym.workspace(cwd);
    use_in(sym, &deps, name, global, update).await
}

async fn use_in(
    sym: &mut Symposium,
    deps: &Arc<Workspace>,
    name: &str,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let pms = deps.pms();
    let registry = crate::plugins::load_registry(sym).await;
    let found = match pms.find(&registry.plugins, name, UpdateLevel::Check).await {
        Ok(found) => found,
        Err(e) => bail!("{e} (try `symposium search {name}`)"),
    };
    let id = pms.identity(&found).await?;

    // `disable` outranks every way of enabling a plugin, `use` included, so
    // recording an entry here would report success and then install nothing.
    // Re-enabling is dropping the `disable` entry first with `use --remove`,
    // a deliberate step rather than something `use` undoes on the user's behalf.
    if sym.config.plugins.is_disabled(pms, &found).await {
        if sym.config.plugins.disable.iter().any(|entry| entry == "*") {
            bail!(
                "`{name}` is disabled by `\"*\"` in `[plugins] disable`; remove that entry to enable it"
            );
        }
        bail!(
            "`{name}` is disabled in `[plugins] disable`; `symposium use {name} --remove` drops that entry"
        );
    }

    // A configured registry is a trust root: what it offers is already
    // enabled by configuration, so there is nothing to record. The exceptions
    // are a dormant plugin, for which `use` is exactly the wake-up call, and
    // `--global`, which also installs the plugin for the user.
    let registry_plugin = registry_plugin_of(pms, &registry.plugins, &id).await?;
    let dormant = registry_plugin.is_some_and(|p| p.manifest.requires_use);
    let already_trusted = registry_plugin.is_some() && !dormant && !global;
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

    // A revocation outranks `use` just as `disable` does: the entry would
    // report success and load nothing. Checked against the release that would
    // load, the same check every load route makes.
    if registry_plugin.is_none() {
        let audits = crate::audits::AuditIndex::configured(sym);
        for plugin in pms.load_plugin(&found, UpdateLevel::Check).await {
            if let Some(revocation) = audits.revocation(pms, &plugin.canonical).await {
                bail!("`{name}` is {revocation}; it cannot be enabled");
            }
        }
    }

    let workspace_root = deps.root().await.map(Path::to_path_buf);
    if !global && let Err(e) = deps.require_info().await {
        bail!("{e}; pass --global to enable `{name}` everywhere");
    }

    let recorded = registry_plugin.map_or(&found.name, |p| &p.manifest.name);
    let entry = match &workspace_root {
        _ if global => UseEntry::Global(recorded.clone()),
        Some(root) => UseEntry::Workspace {
            name: recorded.clone(),
            workspace: root.clone(),
        },
        None => unreachable!("checked above"),
    };

    let mut already_enabled = false;
    for e in &sym.config.plugins.used {
        if in_scope(e, global, workspace_root.as_deref())
            && names(pms, &registry.plugins, e.name(), name, Some(&id)).await
        {
            already_enabled = true;
            break;
        }
    }
    if already_enabled {
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
                name: recorded.clone(),
                global,
            },
        );
    }

    // Install now rather than waiting for the next sync.
    if workspace_root.is_some() {
        crate::sync::sync(sym, deps, update).await?;
    } else {
        crate::sync::sync_user_plugins(sym, deps, update, &BTreeSet::new()).await?;
    }
    Ok(())
}

/// Drop a previously recorded enablement and re-sync so the plugin's skills
/// are reaped now.
///
/// The scope must match: without `--global` this removes the entry recorded
/// for the current workspace, with it the unscoped entry. The consent prompt's
/// answer for the name, its `auto-enable` or `disable` entry, has no scope and
/// is dropped either way, so the dependency is offered again. Finding no entry
/// at all is an error rather than a silent success.
pub async fn remove_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let deps = sym.workspace(cwd);
    let workspace_root = deps.root().await.map(Path::to_path_buf);
    let pms = deps.pms();
    let registry = crate::plugins::load_registry(sym).await;
    let found = pms
        .find(&registry.plugins, name, UpdateLevel::Check)
        .await
        .ok();
    let id = match &found {
        Some(found) => pms.identity(found).await.ok(),
        None => None,
    };

    let used = std::mem::take(&mut sym.config.plugins.used);
    let before = used.len();
    let mut removed_global = Vec::new();
    for entry in used {
        let remove = in_scope(&entry, global, workspace_root.as_deref())
            && names(pms, &registry.plugins, entry.name(), name, id.as_ref()).await;
        if !remove {
            sym.config.plugins.used.push(entry);
        } else if let UseEntry::Global(stored) = entry {
            removed_global.push(stored);
        }
    }
    let removed_use = sym.config.plugins.used.len() != before;

    let mut answered: Vec<PackageId> = found.iter().cloned().collect();
    for dep in deps.dep_ids().await {
        let dep = PackageId::any_version(&dep.pm, &dep.name);
        if !answered.contains(&dep)
            && crate::config::names_match(pms, std::iter::once(name), &dep)
                .await
                .unwrap_or(false)
        {
            answered.push(dep);
        }
    }
    let plugins = &mut sym.config.plugins;
    let mut forgotten = BTreeSet::new();
    for (list, key) in [
        (&mut plugins.auto_enable, "auto-enable"),
        (&mut plugins.disable, "disable"),
    ] {
        for entry in std::mem::take(list) {
            if answer_names(pms, &entry, name, &answered).await {
                forgotten.insert(key);
            } else {
                list.push(entry);
            }
        }
    }

    if !removed_use && forgotten.is_empty() {
        let scope = if global { "--global" } else { "this workspace" };
        bail!(
            "no `use` entry for `{name}` ({scope}), nor an `auto-enable` or `disable` one; see `symposium status`"
        );
    }
    sym.save_config().context("failed to write user config")?;
    for stored in &removed_global {
        crate::state::forget_trial_use(sym.config_dir(), stored);
    }
    if removed_use {
        tracing::info!(
            report = %ReportEvent::PluginRemoved {
                name: name.to_string(),
                global,
            },
        );
    }
    for key in &forgotten {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("removed `{name}` from `[plugins] {key}`"),
            },
        );
    }

    let registry_plugin = match &id {
        Some(id) => registry_plugin_of(pms, &registry.plugins, id)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let mut disabled = match (registry_plugin, &found) {
        (Some(plugin), _) => crate::sync::declared_mcp_servers([plugin]),
        (None, Some(found)) => {
            crate::sync::declared_mcp_servers(&pms.load_plugin(found, UpdateLevel::None).await)
        }
        (None, None) => BTreeSet::new(),
    };
    if !forgotten.is_empty() {
        for dep in answered.iter().filter(|dep| Some(*dep) != found.as_ref()) {
            disabled.extend(crate::sync::declared_mcp_servers(
                &pms.load_plugin(dep, UpdateLevel::None).await,
            ));
        }
    }
    if workspace_root.is_some() {
        crate::sync::sync_disabling(sym, &deps, update, &disabled).await?;
    } else {
        crate::sync::sync_user_plugins(sym, &deps, update, &disabled).await?;
    }
    Ok(())
}

async fn registry_plugin_of<'a>(
    pms: &PmRegistry,
    plugins: &'a [Plugin],
    id: &PackageId,
) -> Result<Option<&'a Plugin>> {
    for plugin in plugins {
        if pms.identity(&plugin.canonical).await? == *id {
            return Ok(Some(plugin));
        }
    }
    Ok(None)
}

/// Does the `auto-enable` or `disable` entry `entry` name one of the packages
/// `answered` that `name` resolved to, compared the way those lists are? `"*"`
/// names no package in particular, so it is never one to drop.
async fn answer_names(pms: &PmRegistry, entry: &str, name: &str, answered: &[PackageId]) -> bool {
    if entry == "*" {
        return false;
    }
    if entry == name {
        return true;
    }
    for id in answered {
        if crate::config::names_match(pms, std::iter::once(entry), id)
            .await
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

fn in_scope(entry: &UseEntry, global: bool, workspace_root: Option<&Path>) -> bool {
    match entry {
        UseEntry::Global(_) => global,
        UseEntry::Workspace { .. } => {
            !global && workspace_root.is_some_and(|root| entry.applies_in(root))
        }
    }
}

/// Does the `use` entry `entry` name the package `id` that `name` resolved
/// to? An entry or a name that no package manager has matches only as written.
async fn names(
    pms: &PmRegistry,
    plugins: &[Plugin],
    entry: &str,
    name: &str,
    id: Option<&PackageId>,
) -> bool {
    entry == name
        || (id.is_some()
            && pms
                .identify(plugins, entry, UpdateLevel::Check)
                .await
                .ok()
                .as_ref()
                == id)
}

#[cfg(test)]
mod tests;
