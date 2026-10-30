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
//! [`PluginManifest::requires_use`](crate::plugins::PluginManifest::requires_use)).
//!
//! `use` only adds to what *may* run; activation predicates still decide
//! when it applies. `--remove` is the inverse, and re-syncs so the plugin's
//! skills are reaped straight away.

use std::path::Path;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::config::{Symposium, UseEntry};
use crate::discovery::Names;
use crate::plugins::Plugin;
use crate::pm::{PackageId, PmRegistry};
use crate::report::ReportEvent;

/// Every plugin a name could mean, from the nearest place that has one.
///
/// Registry plugins first, then the workspace's dependencies, the order a bare
/// `use` entry is read in ([`Names`](crate::discovery::Names)); then a search of
/// the ecosystem transports, which is how `use` names a package the workspace
/// does not depend on yet. The first with a match answers. So waking a
/// recommendation, or enabling a dependency, never reaches the network, and a
/// crates.io crate that happens to share a recommendation's name does not make
/// the name ambiguous.
///
/// Several matches within one are an ambiguity the user resolves with `--pm`,
/// which narrows each.
async fn candidates(
    names: &Names<'_>,
    registry: &[Plugin],
    pms: &PmRegistry,
    name: &str,
    pm: Option<&str>,
) -> Vec<Candidate> {
    let nearest = names.nearest(name, pm).await;
    if !nearest.is_empty() {
        return nearest
            .iter()
            .map(|id| match registry.iter().find(|p| p.canonical == *id) {
                Some(p) => Candidate::new(id, p.manifest.requires_use, true),
                None => Candidate::new(id, false, false),
            })
            .collect();
    }

    // Resolve only the selected transports, using the same cache-first lookup
    // as config enablement. Explicit use may search after a cache miss.
    let mut found = Vec::new();
    for inst in pms
        .instances()
        .filter(|inst| !inst.trusted && pm.is_none_or(|selected| inst.name == selected))
    {
        if let Some(id) = inst.find(name, UpdateLevel::Check).await {
            found.push(Candidate::new(&id, false, false));
        }
    }
    found
}

struct Candidate {
    id: PackageId,
    /// A dormant registry plugin: `use` is exactly its wake-up call.
    dormant: bool,
    /// Offered by a trust root, so it is already enabled unless dormant.
    trusted: bool,
}

impl Candidate {
    fn new(id: &PackageId, dormant: bool, trusted: bool) -> Self {
        Self {
            id: PackageId::any_version(&id.pm, &id.name),
            dormant,
            trusted,
        }
    }
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
    let pms = deps.pms();
    let registry = crate::plugins::load_registry(sym).await;
    let names = Names::new(&registry.plugins, deps.dep_ids().await, pms);
    let mut found = candidates(&names, &registry.plugins, pms, name, pm).await;

    let chosen = match found.len() {
        0 => match pm {
            Some(pm) => bail!("no package manager `{pm}` offers `{name}`"),
            None => bail!("no package named `{name}` (try `cargo agents search {name}`)"),
        },
        1 => found.remove(0),
        _ => {
            let mut msg = format!("`{name}` is offered by more than one package manager:\n");
            for c in &found {
                msg.push_str(&format!("  --pm {}\n", c.id.pm));
            }
            msg.push_str("pick one with `--pm <name>`");
            bail!(msg);
        }
    };

    // `disable` outranks every way of enabling a plugin, `use` included, so
    // recording an entry here would report success and then install nothing.
    // Re-enabling is dropping the `disable` entry, which is a deliberate edit
    // rather than something `use` undoes on the user's behalf.
    let mut spellings = names.spellings(&sym.config.plugins).await;
    names
        .spell(&sym.config.plugins, &chosen.id, &mut spellings)
        .await;
    if sym.config.plugins.is_disabled(&chosen.id, &spellings) {
        bail!("`{name}` is disabled in `[plugins] disable`; remove that entry to enable it again");
    }

    // A trust root's plugins are already enabled by configuration, so there is
    // nothing to record. A dormant one is the exception.
    if chosen.trusted && !chosen.dormant {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "`{name}` is already available from registry `{}`; nothing to enable \
                     (pass `--pm` to name a package of another package manager)",
                    chosen.id.pm
                ),
            },
        );
        return Ok(());
    }

    let workspace_root = deps.root().await.map(Path::to_path_buf);
    if !global && let Err(e) = deps.require_info().await {
        bail!("{e}; pass --global to enable `{name}` everywhere");
    }

    let plugin = crate::config::PluginRef::new(&chosen.id.pm, &chosen.id.name);
    let entry = match &workspace_root {
        _ if global => UseEntry::global(plugin),
        Some(root) => UseEntry::scoped(plugin, root.clone()),
        None => unreachable!("checked above"),
    };

    let mut already_enabled = false;
    for e in &sym.config.plugins.used {
        if in_scope(e, global, workspace_root.as_deref())
            && let Some(plugin) = resolved_entry(&names, pms, e).await
            && plugin.matches(&chosen.id)
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
/// an error rather than a silent success. `pm` narrows the removal to one
/// package manager's entry when several name the same plugin; a bare entry
/// counts as the package manager it resolves to.
pub async fn remove_plugin(
    sym: &mut Symposium,
    cwd: &Path,
    name: &str,
    pm: Option<&str>,
    global: bool,
    update: UpdateLevel,
) -> Result<()> {
    let deps = sym.workspace(cwd);
    let workspace_root = deps.root().await.map(Path::to_path_buf);
    let pms = deps.pms();
    let registry = crate::plugins::load_registry(sym).await;
    let names = Names::new(&registry.plugins, deps.dep_ids().await, pms);

    let used = std::mem::take(&mut sym.config.plugins.used);
    let before = used.len();
    for entry in used {
        let remove = in_scope(&entry, global, workspace_root.as_deref())
            && entry_names(&names, pms, &entry, name, pm).await;
        if !remove {
            sym.config.plugins.used.push(entry);
        }
    }
    if sym.config.plugins.used.len() == before {
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

fn in_scope(entry: &UseEntry, global: bool, workspace_root: Option<&Path>) -> bool {
    if entry.is_global() {
        global
    } else {
        !global && workspace_root.is_some_and(|root| entry.applies_in(root))
    }
}

/// Resolve an existing entry without reaching the network. Bare entries for
/// cached non-dependencies need the same lookup as config enablement, so they
/// can be recognized by `use` and removed with `--pm`.
async fn resolved_entry(
    names: &Names<'_>,
    pms: &PmRegistry,
    entry: &UseEntry,
) -> Option<crate::config::PluginRef> {
    match names.qualify(entry.plugin()).await {
        Ok(Some(plugin)) => Some(plugin),
        Ok(None) => crate::discovery::find_unknown(
            pms,
            std::slice::from_ref(entry.plugin()),
            UpdateLevel::None,
        )
        .await
        .into_iter()
        .next(),
        Err(_) => None,
    }
}

/// Does the `use` entry `entry` name `name`, as its package manager reads
/// names? A hand-written bare entry counts as the package manager it resolves
/// to, for `--pm` too.
async fn entry_names(
    names: &Names<'_>,
    pms: &PmRegistry,
    entry: &UseEntry,
    name: &str,
    pm: Option<&str>,
) -> bool {
    let resolved = resolved_entry(names, pms, entry).await.and_then(|r| r.pm);
    let same = match &resolved {
        Some(p) => pms.canonical_name(p, entry.name()).await == pms.canonical_name(p, name).await,
        None => entry.name() == name,
    };
    same && pm.is_none_or(|pm| resolved.as_deref() == Some(pm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::{PackageManager, PluginKind, PmInstance, UnvalidatedPlugin};
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn use_finds_cached_packages_before_searching() {
        let (cargo, network) = crate::test_utils::FakeCargo::instance(&["widget-lib"], &[]);
        let pms = PmRegistry::new(vec![cargo]);
        let names = Names::new(&[], &[], &pms);
        let found = candidates(&names, &[], &pms, "widget_lib", None).await;
        assert_eq!(found.len(), 1, "a cached package must be usable offline");
        assert_eq!(found[0].id, PackageId::any_version("cargo", "widget-lib"));
        assert_eq!(network.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_bare_cached_use_entry_can_be_recognized_and_removed_by_pm() {
        let (cargo, network) = crate::test_utils::FakeCargo::instance(&["widget-lib"], &[]);
        let pms = PmRegistry::new(vec![cargo]);
        let names = Names::new(&[], &[], &pms);
        let entry = UseEntry::global(crate::config::PluginRef::bare("widget_lib"));
        assert_eq!(
            resolved_entry(&names, &pms, &entry).await,
            Some(crate::config::PluginRef::new("cargo", "widget-lib"))
        );
        assert!(entry_names(&names, &pms, &entry, "widget-lib", Some("cargo")).await);
        assert!(!entry_names(&names, &pms, &entry, "widget-lib", Some("npm")).await);
        assert_eq!(network.load(Ordering::SeqCst), 0);
    }

    struct UnrelatedPm;

    #[async_trait::async_trait]
    impl PackageManager for UnrelatedPm {
        fn name(&self) -> &str {
            "unrelated"
        }

        async fn load_plugin(
            &self,
            _: &PackageId,
            _: UpdateLevel,
        ) -> Result<Vec<UnvalidatedPlugin>> {
            unreachable!("a qualified lookup must not load from another PM")
        }

        async fn search(&self, _: &str) -> Result<Vec<crate::pm::PluginInfo>> {
            panic!("a qualified lookup must not search another PM")
        }
    }

    #[tokio::test]
    async fn use_searches_only_the_selected_package_manager() {
        let (cargo, network) = crate::test_utils::FakeCargo::instance(&[], &["widget-lib"]);
        let unrelated = PmInstance::new(
            "unrelated",
            false,
            PluginKind::Package,
            Box::new(UnrelatedPm),
        );
        let pms = PmRegistry::new(vec![unrelated, cargo]);
        let names = Names::new(&[], &[], &pms);
        let found = candidates(&names, &[], &pms, "widget_lib", Some("cargo")).await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, PackageId::any_version("cargo", "widget-lib"));
        assert_eq!(network.load(Ordering::SeqCst), 1);
    }
}
