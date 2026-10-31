//! `cargo agents status` — the enablement report.
//!
//! Enablement answers *whether a plugin may run at all*; activation
//! predicates answer *when it applies*. This command reports both, one line
//! per plugin, each naming its enablement root — so it answers "why is
//! serde-skills here?" with "enabled via serde".
//!
//! Four states, matching the axis:
//!
//! - **active** — enabled and its predicates hold for this workspace. The
//!   root names the trust root: workspace membership, a configured registry,
//!   `[plugins] auto-enable`, or a `[plugins] use` entry.
//! - **dormant** — loaded but waiting: a registry plugin that names no
//!   dependency ([`requires_use`](crate::plugins::PluginManifest::requires_use)),
//!   one whose predicates don't currently hold, or a dependency named by `use`
//!   that offers no plugin.
//! - **candidate** — discovered in a dependency and awaiting consent. These
//!   are exactly what the [consent prompt](crate::discovery::prompt_for_consent)
//!   asks about.
//! - **declined** — recorded in `[plugins] disable`, the record of pruned
//!   nodes and declined discoveries.

use std::path::Path;

use crate::pm::WorkspaceDeps;
use anyhow::Result;
use std::sync::Arc;
use symposium_install::UpdateLevel;

use crate::config::Symposium;
use crate::discovery::{DiscoveredPlugin, Enablement};
use crate::report::ReportEvent;

/// What enablement decided about one plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusState {
    /// Enabled, and its predicates hold here.
    Active,
    /// Loaded but not contributing: awaiting `use`, or predicates unmet.
    Dormant,
    /// Discovered in a dependency, awaiting consent.
    Candidate,
    /// Declined, via `[plugins] disable`.
    Declined,
}

impl StatusState {
    /// The wire/report spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Dormant => "dormant",
            Self::Candidate => "candidate",
            Self::Declined => "declined",
        }
    }
}

/// One line of the status report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub name: String,
    /// Resolved version for a discovered dependency plugin; `None` for
    /// manifest plugins, whose identity is their source, not a version.
    pub version: Option<String>,
    /// The enablement root, or — for the states that don't have one — why
    /// the plugin will not load.
    pub root: String,
    pub state: StatusState,
}

/// Compute the enablement report for the workspace `deps` points at.
pub async fn workspace_status(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
) -> Result<Vec<StatusEntry>> {
    let ws = deps.require()?.clone();

    let mut entries = Vec::new();

    // Manifest plugins: workspace members and registry offerings. Both are
    // trust roots, so the only questions are the `use` gate for dormant
    // plugins and whether the predicates hold.
    let registry = crate::plugins::load_registry_with_workspace(sym, Some(&ws)).await;
    let dep_ids = crate::pm::workspace_dep_ids(sym, deps).await;
    let pms = sym.package_managers(deps);
    let used = crate::discovery::used_plugins(sym, &pms, &registry.plugins, &ws.root);
    let mut ctx = crate::predicate::PredicateContext::new(&dep_ids).with_used(&used);
    for parsed in &registry.plugins {
        let root = if parsed.is_workspace_member() {
            "workspace member".to_string()
        } else if parsed.manifest.requires_use && ctx.is_used(&parsed.package.id) {
            "`[plugins] use`".to_string()
        } else {
            format!("registry `{}`", parsed.package.id.pm)
        };
        let active = parsed.applies(&mut ctx);
        entries.push(StatusEntry {
            name: parsed.manifest.name.clone(),
            version: None,
            root: if active || !parsed.manifest.requires_use {
                root
            } else {
                format!("{root} (dormant: awaiting `cargo agents use`)")
            },
            state: if active {
                StatusState::Active
            } else {
                StatusState::Dormant
            },
        });
    }

    // Dependency-embedded plugins, with the decision the config made about
    // each. This is the same view the consent prompt works from.
    let discovery = crate::discovery::discover(sym, deps, &registry.plugins).await;
    for found in discovery
        .active
        .iter()
        .chain(&discovery.auto_enabled)
        .chain(&discovery.candidates)
        .chain(&discovery.declined)
    {
        entries.push(entry_for(found));
    }

    // Crates enabled by `use` that are *not* dependency-embedded offers — e.g.
    // `cargo agents use <crate>` for a crate the workspace doesn't depend on.
    // Discovery only sees dependencies, so these are invisible above; surface
    // them from the config the same way sync loads them
    // ([`enabled_dependencies`](crate::discovery::enabled_dependencies)).
    let shown: std::collections::HashSet<crate::pm::PackageId> = registry
        .plugins
        .iter()
        .map(|p| pms.identity(&p.package.id))
        .chain(
            discovery
                .active
                .iter()
                .chain(&discovery.auto_enabled)
                .chain(&discovery.candidates)
                .chain(&discovery.declined)
                .map(|d| pms.identity(&d.id)),
        )
        .collect();
    let enabled = crate::discovery::enabled_dependencies(
        sym,
        &pms,
        &registry.plugins,
        &dep_ids,
        &ws.root,
        UpdateLevel::None,
    )
    .await;
    let used =
        crate::discovery::resolve_used(sym, &pms, &registry.plugins, &ws.root, UpdateLevel::None)
            .await;
    for id in enabled {
        if shown.contains(&pms.identity(&id)) {
            continue;
        }
        let dependency = dep_ids
            .iter()
            .any(|dep| pms.identity(dep) == pms.identity(&id));
        let named = used.iter().any(|(_, used)| *used == pms.identity(&id));
        if dependency && !named {
            continue;
        }
        entries.push(StatusEntry {
            name: id.name,
            version: None,
            root: if dependency {
                "`[plugins] use` (offers no plugin)"
            } else {
                "`[plugins] use` (not a dependency)"
            }
            .to_string(),
            state: if dependency {
                StatusState::Dormant
            } else {
                StatusState::Active
            },
        });
    }
    let mut missing = std::collections::HashSet::new();
    for name in sym.config.plugins.used_names_in(&ws.root) {
        if !missing.insert(name)
            || pms
                .identify(&registry.plugins, name, UpdateLevel::None)
                .await
                .is_ok()
        {
            continue;
        }
        entries.push(StatusEntry {
            name: name.to_string(),
            version: None,
            root: "`[plugins] use` (not a registry plugin, a dependency or a cached crate)"
                .to_string(),
            state: StatusState::Dormant,
        });
    }

    // Names declined without ever being discovered (a `disable` entry for a
    // dependency whose source isn't on disk, or one added by hand). A name no
    // package manager has can only be compared as written.
    let mut declined: std::collections::HashSet<crate::pm::PackageId> = discovery
        .declined
        .iter()
        .map(|d| pms.identity(&d.id))
        .collect();
    let mut unknown = std::collections::HashSet::new();
    for name in &sym.config.plugins.disable {
        if name == "*" {
            continue;
        }
        let first = match pms
            .identify(&registry.plugins, name, UpdateLevel::None)
            .await
        {
            Ok(id) => declined.insert(id),
            Err(_) => unknown.insert(name.as_str()),
        };
        if !first {
            continue;
        }
        entries.push(StatusEntry {
            name: name.clone(),
            version: None,
            root: "declined (`[plugins] disable`)".to_string(),
            state: StatusState::Declined,
        });
    }

    Ok(entries)
}

/// Render one discovered dependency plugin as a status line.
fn entry_for(found: &DiscoveredPlugin) -> StatusEntry {
    let (state, root) = match found.enablement {
        Enablement::Used => (StatusState::Active, "`[plugins] use`".to_string()),
        Enablement::AutoEnabled => (StatusState::Active, "`[plugins] auto-enable`".to_string()),
        Enablement::Declined => (
            StatusState::Declined,
            "declined (`[plugins] disable`)".to_string(),
        ),
        Enablement::Candidate => (
            StatusState::Candidate,
            format!(
                "found via dependency `{}`, awaiting consent (`cargo agents use {}`)",
                found.recommends,
                found.name()
            ),
        ),
    };
    StatusEntry {
        name: found.name().to_string(),
        version: Some(found.id.version.clone()),
        root,
        state,
    }
}

/// The `cargo agents status` entry point.
pub async fn status(sym: &Symposium, cwd: &Path) -> Result<()> {
    let deps = sym.workspace_deps(cwd);
    let entries = workspace_status(sym, &deps).await?;
    if entries.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: "no plugins enabled for this workspace".to_string(),
            },
        );
        return Ok(());
    }
    for entry in entries {
        tracing::info!(
            report = %ReportEvent::PluginStatus {
                name: entry.name,
                version: entry.version,
                root: entry.root,
                state: entry.state.as_str().to_string(),
            },
        );
    }
    Ok(())
}
