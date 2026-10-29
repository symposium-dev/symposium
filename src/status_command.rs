//! `cargo agents status` — the enablement report.
//!
//! Enablement answers *whether a plugin may run at all*; activation
//! predicates answer *when it applies*. This command reports both, one line
//! per plugin, each naming its enablement root — so it answers "why is
//! serde-skills here?" with "enabled via serde".
//!
//! Five states, matching the axis:
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
//! - **revoked** — the [audits list](crate::audits) revokes this release or
//!   payload, for the stated reason. It is not loaded, whatever `use` or
//!   `auto-enable` say; only `disable` outranks it, in which case the plugin
//!   shows as declined.
//!
//! A dependency plugin's line also says what the audits list says about it —
//! audited, or not audited (and, when the list audits the package but this
//! payload cannot be digested, why) — in the same words as the consent
//! prompt and the `SessionStart` hint. An audit changes nothing about what
//! loads.

use std::path::Path;

use crate::pm::{PackageId, Workspace};
use anyhow::Result;
use std::sync::Arc;
use symposium_install::UpdateLevel;

use crate::audits::{Answer, AuditIndex, Verdict};
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
    /// Revoked by the audits list.
    Revoked,
}

impl StatusState {
    /// The wire/report spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Dormant => "dormant",
            Self::Candidate => "candidate",
            Self::Declined => "declined",
            Self::Revoked => "revoked",
        }
    }
}

/// One line of the status report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub name: String,
    pub version: Option<String>,
    /// The enablement root, or — for the states that don't have one — why
    /// the plugin will not load.
    pub root: String,
    pub state: StatusState,
    pub format: Option<&'static str>,
    /// What the audits list says about a dependency plugin; `None` when no
    /// list was read, when it is revoked (a revoked line's root says so, a
    /// declined one stays declined) or not looked up: a trust root's plugin,
    /// a `use`d crate that is not cached yet or a dependency that offers no
    /// plugin, and a declined name discovery never saw.
    pub audit: Option<Verdict>,
}

/// Compute the enablement report for the workspace `deps` points at.
pub async fn workspace_status(
    sym: &Symposium,
    workspace: &Arc<Workspace>,
) -> Result<Vec<StatusEntry>> {
    let ws = workspace.require_info().await?.clone();

    let mut entries = Vec::new();

    // Manifest plugins: workspace members and registry offerings. Both are
    // trust roots, so the only questions are the `use` gate for dormant
    // plugins and whether the predicates hold.
    let registry = crate::plugins::load_registry_with_workspace(sym, Some(&ws)).await;
    let dep_ids = workspace.dep_ids().await.to_vec();
    let pms = workspace.pms();
    let names = crate::discovery::Names::new(&registry.plugins, &dep_ids, pms);
    let used = crate::discovery::used(sym, &ws.root, &names).await;
    let spellings = names.spellings(&sym.config.plugins).await;
    let mut ctx = crate::predicate::PredicateContext::new(&dep_ids).with_used(&used.plugins);
    for parsed in &registry.plugins {
        let root = if parsed.workspace_member {
            "workspace member".to_string()
        } else if parsed.manifest.requires_use && ctx.is_used(&parsed.canonical) {
            "`[plugins] use`".to_string()
        } else {
            format!("registry `{}`", parsed.canonical.pm)
        };
        let active = parsed.applies(&mut ctx);
        entries.push(StatusEntry {
            name: parsed.manifest.name.clone(),
            version: parsed.manifest.version.clone(),
            format: parsed.format(),
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
            audit: None,
        });
    }

    // Dependency-embedded plugins, with the decision the config made about
    // each. This is the same view the consent prompt works from.
    let discovery = crate::discovery::discover(sym, workspace, &used.plugins, &spellings).await;
    for found in discovery
        .active
        .iter()
        .chain(&discovery.auto_enabled)
        .chain(&discovery.candidates)
        .chain(&discovery.declined)
        .chain(&discovery.revoked)
    {
        entries.push(entry_for(found));
    }

    // Crates enabled by `use` that are *not* dependency-embedded offers — e.g.
    // `cargo agents use <crate>` for a crate the workspace doesn't depend on.
    // Discovery only sees dependencies, so these are invisible above; surface
    // them from the config the same way sync loads them
    // ([`enabled_dependencies`](crate::discovery::enabled_dependencies)).
    let shown: std::collections::HashSet<PackageId> = registry
        .plugins
        .iter()
        .map(|p| pms.identity(&p.canonical))
        .chain(
            discovery
                .active
                .iter()
                .chain(&discovery.auto_enabled)
                .chain(&discovery.candidates)
                .chain(&discovery.declined)
                .chain(&discovery.revoked)
                .map(|d| pms.identity(&d.id)),
        )
        .collect();
    let enabled = crate::discovery::enabled_dependencies(
        sym,
        pms,
        &dep_ids,
        &used,
        &spellings,
        UpdateLevel::None,
    )
    .await;
    for id in enabled {
        let identity = pms.identity(&id);
        if shown.contains(&identity) {
            continue;
        }
        let dependency = dep_ids
            .iter()
            .any(|dep| pms.identity(dep) == pms.identity(&id));
        let named = used.plugins.iter().any(|entry| entry.matches(&id));
        if dependency && !named {
            continue;
        }
        // A `use`d crate the workspace does not depend on is loaded from
        // cargo's cache, so its release is known without fetching: the audits
        // apply to it like to any other dependency plugin.
        let mut audit = None;
        if !dependency && let Ok(fetched) = pms.fetch(&id, UpdateLevel::None).await {
            let audits = AuditIndex::configured(sym);
            match audits.answer(pms, &fetched.id).await {
                Answer::Revoked(revocation) => {
                    entries.push(StatusEntry {
                        name: id.name,
                        version: Some(fetched.id.version),
                        root: revocation.to_string(),
                        state: StatusState::Revoked,
                        format: None,
                        audit: None,
                    });
                    continue;
                }
                // As for discovered plugins: no verdict without a list read.
                Answer::Verdict(verdict) => audit = audits.is_read().then_some(verdict),
            }
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
            format: None,
            audit,
        });
    }
    // Bare `use` names nothing here has: no registry plugin, dependency, or
    // package on disk.
    let mut missing = std::collections::HashSet::new();
    for entry in &used.unknown {
        if !missing.insert(&entry.name) {
            continue;
        }
        let mut found = false;
        for inst in pms.instances() {
            found |= inst.find(&entry.name, UpdateLevel::None).await.is_some();
        }
        if found {
            continue;
        }
        entries.push(StatusEntry {
            name: entry.name.clone(),
            version: None,
            root: "`[plugins] use` (not a registry plugin, a dependency or a cached crate)"
                .to_string(),
            state: StatusState::Dormant,
            format: None,
            audit: None,
        });
    }

    // `use` entries whose bare name could mean more than one plugin, which
    // enable nothing until they name a package manager.
    for (entry, pms) in &used.ambiguous {
        entries.push(StatusEntry {
            name: entry.name.clone(),
            version: None,
            root: ambiguous_use(entry, pms),
            state: StatusState::Dormant,
        });
    }

    // Names declined without ever being discovered (a `disable` entry for a
    // dependency whose source isn't on disk, or one added by hand), each once:
    // two entries the same package manager would read as one name are one.
    let mut listed: std::collections::HashSet<PackageId> = discovery
        .declined
        .iter()
        .map(|d| pms.identity(&d.id))
        .collect();
    for entry in &sym.config.plugins.disable {
        let shown = discovery
            .declined
            .iter()
            .any(|d| spellings.names(entry, &d.id));
        if entry.name == "*" || shown {
            continue;
        }
        let key = match names.qualify(entry).await {
            Ok(Some(plugin)) => pms.identity(&PackageId::any_version(
                plugin.pm.unwrap_or_default(),
                &plugin.name,
            )),
            Ok(None) | Err(_) => PackageId::any_version("", &entry.name),
        };
        if !listed.insert(key) {
            continue;
        }
        entries.push(StatusEntry {
            name: entry.name.clone(),
            version: None,
            root: "declined (`[plugins] disable`)".to_string(),
            state: StatusState::Declined,
            format: None,
            audit: None,
        });
    }

    Ok(entries)
}

/// Why a `use` entry with an ambiguous bare name enables nothing.
pub fn ambiguous_use(entry: &crate::config::PluginRef, pms: &[String]) -> String {
    format!(
        "`[plugins] use` entry `{entry}` could be from {}; add `pm` to say which",
        pms.iter()
            .map(|pm| format!("`{pm}`"))
            .collect::<Vec<_>>()
            .join(" or ")
    )
}

/// Render one discovered dependency plugin as a status line, with the verdict
/// discovery looked up (none for a revoked offer, declined or not).
fn entry_for(found: &DiscoveredPlugin) -> StatusEntry {
    let (state, root) = match &found.enablement {
        Enablement::Used => (StatusState::Active, "`[plugins] use`".to_string()),
        Enablement::AutoEnabled => (StatusState::Active, "`[plugins] auto-enable`".to_string()),
        Enablement::Declined => (
            StatusState::Declined,
            "declined (`[plugins] disable`)".to_string(),
        ),
        Enablement::Revoked(revocation) => (StatusState::Revoked, revocation.to_string()),
        Enablement::Candidate => (
            StatusState::Candidate,
            format!(
                "found via dependency `{}`, awaiting consent (`{}`)",
                found.recommends,
                crate::discovery::use_command(&crate::config::PluginRef::new(
                    &found.id.pm,
                    &found.id.name
                ))
            ),
        ),
    };
    StatusEntry {
        name: found.name().to_string(),
        version: Some(found.id.version.clone()),
        root,
        state,
        format: found.format,
        audit: found.audit.clone(),
    }
}

/// The `cargo agents status` entry point.
pub async fn status(sym: &Symposium, cwd: &Path) -> Result<()> {
    let deps = sym.workspace(cwd);
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
                format: entry.format.map(str::to_string),
                audit: entry.audit.as_ref().map(Verdict::to_string),
            },
        );
    }
    Ok(())
}
