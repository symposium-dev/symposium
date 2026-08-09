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
use crate::report::ReportEvent;
use symposium_pm_cargo::sources::normalize_crate_name;

/// Every plugin a name could mean, across every package manager.
///
/// A name is not an identity, so `use` has to look everywhere and then decide.
/// One match is unambiguous and is used; several is an error the user resolves
/// with `--pm`.
async fn candidates(
    sym: &Symposium,
    ws: &std::sync::Arc<crate::pm::Workspace>,
    name: &str,
) -> Vec<Candidate> {
    let normalized = normalize_crate_name(name);
    let mut out: Vec<Candidate> = Vec::new();
    // Two sources may spell the same package differently (`crate-a` against
    // `crate_a`), so identity here is the normalized name, as everywhere else.
    let mut push = |plugin: crate::config::PluginRef, dormant: bool, trusted: bool| {
        let same = |c: &Candidate| {
            c.plugin.pm == plugin.pm
                && normalize_crate_name(&c.plugin.name) == normalize_crate_name(&plugin.name)
        };
        if !out.iter().any(same) {
            out.push(Candidate {
                plugin,
                dormant,
                trusted,
            });
        }
    };

    // Registry plugins, which name themselves.
    for parsed in &crate::plugins::load_registry(sym).await.plugins {
        if normalize_crate_name(&parsed.plugin.name) == normalized {
            push(
                crate::config::PluginRef::new(&parsed.canonical.pm, &parsed.canonical.name),
                parsed.plugin.requires_use,
                true,
            );
        }
    }

    // Workspace dependencies, checked offline before anything reaches out.
    for id in ws.dep_ids().await {
        if normalize_crate_name(&id.name) == normalized {
            push(
                crate::config::PluginRef::new(&id.pm, &id.name),
                false,
                false,
            );
        }
    }

    // Anything a package manager can find that the workspace does not depend
    // on, which is how `use` names a crate you have not added yet.
    for (_, info) in ws.pms().search(name).await {
        if normalize_crate_name(&info.id.name) == normalized {
            push(
                crate::config::PluginRef::new(&info.id.pm, &info.id.name),
                false,
                false,
            );
        }
    }
    out
}

struct Candidate {
    plugin: crate::config::PluginRef,
    /// A dormant registry plugin: `use` is exactly its wake-up call.
    dormant: bool,
    /// Offered by a trust root, so it is already enabled unless dormant.
    trusted: bool,
}

/// Record an enablement for `name` and sync so its skills install now.
///
/// `pm` picks between package managers when more than one offers the name.
pub async fn use_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    pm: Option<&str>,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let deps = sym.workspace(cwd);
    let mut found = candidates(sym, &deps, name).await;
    if let Some(pm) = pm {
        found.retain(|c| c.plugin.pm == pm);
    }

    let chosen = match found.len() {
        0 => match pm {
            Some(pm) => bail!("no package manager `{pm}` offers `{name}`"),
            None => bail!("no crate or plugin named `{name}`; try `cargo agents search {name}`"),
        },
        1 => found.remove(0),
        _ => {
            let mut msg = format!("`{name}` is offered by more than one package manager:\n");
            for c in &found {
                msg.push_str(&format!("  {} (--pm {})\n", c.plugin.pm, c.plugin.pm));
            }
            msg.push_str("pick one with `--pm <name>`");
            bail!(msg);
        }
    };

    // A trust root's plugins are already enabled by configuration, so there is
    // nothing to record. A dormant one is the exception.
    if chosen.trusted && !chosen.dormant {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "`{name}` is already available from a configured registry; nothing to enable"
                ),
            },
        );
        return Ok(());
    }

    let workspace_root = deps.root().await.map(Path::to_path_buf);
    if !global && workspace_root.is_none() {
        bail!("not in a Rust workspace; pass --global to enable `{name}` everywhere");
    }

    let entry = match &workspace_root {
        _ if global => UseEntry::global(chosen.plugin),
        Some(root) => UseEntry::scoped(chosen.plugin, root.clone()),
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
        crate::sync::sync(sym, &deps, update).await?;
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
    let deps = sym.workspace(cwd);
    let workspace_root = deps.root().await.map(Path::to_path_buf);

    let used = &mut sym.config.plugins.used;
    let before = used.len();
    used.retain(|entry| {
        if normalize_crate_name(entry.name()) != normalize_crate_name(name) {
            return true;
        }
        let in_scope = if entry.is_global() {
            global
        } else {
            {
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
        crate::sync::sync(sym, &deps, update).await?;
    }
    Ok(())
}
