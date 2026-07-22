//! Dependency discovery: which plugins the workspace's dependencies bring
//! within reach, and which of them the user has consented to.
//!
//! Discovery is the read side of the enablement axis. It runs in two phases:
//!
//! 1. list the workspace's dependencies ([`pm::workspace_dep_ids`]);
//! 2. ask every package-manager instance what plugins it offers
//!    ([`PackageManager::list_plugins`]) and keep the offers that
//!    [recommend](crate::pm::PluginInfo::recommends) one of those
//!    dependencies.
//!
//! Each surviving offer is then classified against the `[plugins]` config:
//! already enabled, auto-enabled, declined, or a candidate still awaiting
//! consent. Nothing here prompts, fetches, or writes — the consent prompt and
//! the commands that record decisions are a separate concern.
//!
//! Enablement matters because a dependency is deliberately *not* a trust
//! root: depending on a crate means compiling its code, not letting its
//! author inject agent context. Registries are trust roots (pointing config
//! at one is the act of trusting its curation), so their offers arrive
//! already enabled.
//!
//! [`pm::workspace_dep_ids`]: crate::pm::workspace_dep_ids
//! [`PackageManager::list_plugins`]: crate::pm::PackageManager::list_plugins

use std::path::Path;

use symposium_sdk::workspace::WorkspaceCrate;

use crate::config::Symposium;
use crate::crate_sources::normalize_crate_name;
use crate::pm::{CARGO_PM, PackageId, PluginInfo, PmContext};

/// Why a discovered offer is (or is not) enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enablement {
    /// Enabled by a `[plugins] use` entry naming it.
    Used,
    /// Offered by a configured registry, which is a trust root.
    Registry,
    /// Enabled ahead of time by `[plugins] auto-enable`.
    AutoEnabled,
    /// Declined: `[plugins] disable` names it.
    Declined,
    /// Nobody has decided yet — this is what a consent prompt would ask about.
    Candidate,
}

impl Enablement {
    /// Does this decision let the plugin run?
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Used | Self::Registry | Self::AutoEnabled)
    }
}

/// One plugin offer whose recommended dependency the workspace has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// The offering package-manager instance: an ecosystem transport
    /// (`cargo`) for a dependency-embedded plugin, or a registry's name.
    pub registry: String,
    /// The offered package.
    pub id: PackageId,
    /// The dependency this offer is a plugin for.
    pub recommends: String,
    /// What the offering PM says the package is, when it says anything.
    pub description: Option<String>,
    /// How the `[plugins]` config decided this offer.
    pub enablement: Enablement,
}

impl DiscoveredPlugin {
    /// The name the user would type to enable this plugin.
    pub fn name(&self) -> &str {
        &self.id.name
    }
}

/// Every dependency-matched offer, grouped by what the config decided.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Already enabled: named by a `use` entry, or offered by a registry.
    pub active: Vec<DiscoveredPlugin>,
    /// Enabled by `[plugins] auto-enable`.
    pub auto_enabled: Vec<DiscoveredPlugin>,
    /// Awaiting consent — newly discovered and not yet decided.
    pub candidates: Vec<DiscoveredPlugin>,
    /// Declined previously, recorded in `[plugins] disable`.
    pub declined: Vec<DiscoveredPlugin>,
}

impl Discovery {
    /// Every offer that may run, whatever enabled it.
    pub fn enabled(&self) -> impl Iterator<Item = &DiscoveredPlugin> {
        self.active.iter().chain(&self.auto_enabled)
    }
}

/// Discover the plugins offered for this workspace's dependencies.
///
/// `workspace_root` scopes the `use` entries that count (an entry can be
/// recorded for one workspace only). Does no network I/O: `list_plugins` is
/// cache-only by contract, so an offer only appears once its content is
/// already on disk.
pub async fn discover(
    sym: &Symposium,
    workspace_crates: &[WorkspaceCrate],
    workspace_root: &Path,
) -> Discovery {
    let dep_ids = crate::pm::workspace_dep_ids(sym, workspace_crates).await;
    let pms = sym.package_managers();
    let cx = PmContext::new(sym, workspace_crates);

    let mut discovery = Discovery::default();
    for inst in pms.instances() {
        let offers = match inst.pm.list_plugins(&dep_ids, &cx).await {
            Ok(offers) => offers,
            Err(e) => {
                tracing::debug!(instance = %inst.name, error = %e, "cannot list plugins");
                continue;
            }
        };
        for offer in offers {
            let Some(recommends) = matched_dependency(&offer, &dep_ids) else {
                continue;
            };
            let from_registry = offer.id.pm != CARGO_PM;
            let enablement = decide(sym, consent_name(&offer), workspace_root, from_registry);
            let discovered = DiscoveredPlugin {
                registry: inst.name.clone(),
                id: offer.id,
                recommends,
                description: offer.description,
                enablement,
            };
            match enablement {
                Enablement::Used | Enablement::Registry => discovery.active.push(discovered),
                Enablement::AutoEnabled => discovery.auto_enabled.push(discovered),
                Enablement::Declined => discovery.declined.push(discovered),
                Enablement::Candidate => discovery.candidates.push(discovered),
            }
        }
    }
    discovery
}

/// The workspace dependencies whose embedded plugins the user has enabled —
/// through `[plugins] auto-enable` or an applicable `use` entry — as crate
/// names to load.
///
/// This reads the config rather than the offer list, so enabling a registry
/// dependency works even though its source isn't on disk to be discovered
/// yet (see [`CargoPm::list_plugins`](crate::pm::CargoPm)). Declined names
/// are pruned.
pub fn enabled_dependencies<'a>(
    sym: &Symposium,
    dep_ids: &'a [PackageId],
    workspace_root: &Path,
) -> Vec<&'a str> {
    let plugins = &sym.config.plugins;
    dep_ids
        .iter()
        .filter(|id| id.pm == CARGO_PM)
        .filter(|id| !plugins.is_disabled(&id.name))
        .filter(|id| {
            plugins.is_auto_enabled(&id.name) || plugins.is_used_in(&id.name, workspace_root)
        })
        .map(|id| id.name.as_str())
        .collect()
}

/// Which workspace dependency an offer recommends a plugin for, if the
/// workspace has it. Ecosystem-agnostic on purpose: `recommends` is a bare
/// name, so an offer from one PM can recommend a plugin for another's
/// package.
fn matched_dependency(offer: &PluginInfo, dep_ids: &[PackageId]) -> Option<String> {
    let recommends = normalize_crate_name(offer.recommends.as_deref()?);
    dep_ids
        .iter()
        .find(|id| normalize_crate_name(&id.name) == recommends)
        .map(|id| id.name.clone())
}

/// Classify one offer against the `[plugins]` config. An explicit decision —
/// `use`, then `disable` — outranks the standing ones, so a name the user
/// declined stays declined even when a registry offers it.
fn decide(sym: &Symposium, name: &str, workspace_root: &Path, from_registry: bool) -> Enablement {
    let plugins = &sym.config.plugins;
    if plugins.is_used_in(name, workspace_root) {
        Enablement::Used
    } else if plugins.is_disabled(name) {
        Enablement::Declined
    } else if from_registry {
        Enablement::Registry
    } else if plugins.is_auto_enabled(name) {
        Enablement::AutoEnabled
    } else {
        Enablement::Candidate
    }
}

/// The name a consent decision about an offer is recorded under: the offered
/// package's own name, or — for a positional registry entry, whose name is a
/// namespaced path like `cargo/serde` — the dependency it recommends.
fn consent_name(offer: &PluginInfo) -> &str {
    match &offer.recommends {
        Some(dep) if offer.subpath.is_some() => dep,
        _ => &offer.id.name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::ANY_VERSION;
    use indoc::indoc;

    /// A workspace with `widget-lib` as a path dependency carrying skills,
    /// plus a plain registry dependency.
    fn workspace(root: &Path) -> Vec<WorkspaceCrate> {
        let widget = root.join("widget-lib");
        std::fs::create_dir_all(widget.join("skills/guidance")).unwrap();
        std::fs::write(widget.join("skills/guidance/SKILL.md"), "").unwrap();
        vec![
            WorkspaceCrate::new(
                "widget-lib".to_string(),
                semver::Version::new(1, 0, 0),
                Some(widget),
            ),
            WorkspaceCrate::new("serde".to_string(), semver::Version::new(1, 0, 210), None),
        ]
    }

    /// A `Symposium` over a fresh config dir with only the given config, and
    /// no built-in registries (so tests see only what they set up).
    fn sym_with(root: &Path, config: &str) -> Symposium {
        let config_dir = root.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "{}\n[defaults]\nsymposium-recommendations = false\nuser-plugins = false\n",
                config
            ),
        )
        .unwrap();
        Symposium::from_dir(&config_dir)
    }

    #[tokio::test]
    async fn undecided_dependency_plugin_is_a_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let crates = workspace(tmp.path());
        let sym = sym_with(tmp.path(), "");

        let found = discover(&sym, &crates, tmp.path()).await;
        assert!(found.active.is_empty());
        assert!(found.auto_enabled.is_empty());
        let names: Vec<&str> = found.candidates.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert_eq!(found.candidates[0].recommends, "widget-lib");
    }

    #[tokio::test]
    async fn auto_enable_moves_a_candidate_to_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let crates = workspace(tmp.path());
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["widget_lib"]
            "#},
        );

        let found = discover(&sym, &crates, tmp.path()).await;
        assert!(found.candidates.is_empty());
        let names: Vec<&str> = found.auto_enabled.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert!(found.enabled().count() == 1);
    }

    #[tokio::test]
    async fn use_entry_and_disable_outrank_the_standing_decisions() {
        let tmp = tempfile::tempdir().unwrap();
        let crates = workspace(tmp.path());

        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["widget-lib"]
            "#},
        );
        let found = discover(&sym, &crates, tmp.path()).await;
        assert_eq!(found.active.len(), 1);
        assert_eq!(found.active[0].enablement, Enablement::Used);

        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["*"]
                disable = ["widget-lib"]
            "#},
        );
        let found = discover(&sym, &crates, tmp.path()).await;
        assert!(found.auto_enabled.is_empty());
        assert_eq!(found.declined.len(), 1);
    }

    /// A `use` entry recorded for another workspace does not enable anything
    /// here.
    #[tokio::test]
    async fn workspace_scoped_use_entries_only_count_in_their_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let crates = workspace(tmp.path());
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = [{ name = "widget-lib", workspace = "/elsewhere" }]
            "#},
        );

        let found = discover(&sym, &crates, tmp.path()).await;
        assert!(found.active.is_empty());
        assert_eq!(found.candidates.len(), 1);
    }

    #[test]
    fn enabled_dependencies_reads_config_not_offers() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["serde"]
                use = ["tokio"]
                disable = ["clap"]
            "#},
        );
        let deps = [
            // A registry dependency, invisible to `list_plugins`, is still
            // enabled by name.
            PackageId::new(CARGO_PM, "serde", "1.0.210"),
            PackageId::new(CARGO_PM, "tokio", "1.0.0"),
            PackageId::new(CARGO_PM, "clap", "4.0.0"),
            PackageId::new(CARGO_PM, "anyhow", "1.0.0"),
            // Not a cargo package: not a crate to load.
            PackageId::new("npm", "serde", ANY_VERSION),
        ];

        assert_eq!(
            enabled_dependencies(&sym, &deps, tmp.path()),
            vec!["serde", "tokio"]
        );
    }
}
