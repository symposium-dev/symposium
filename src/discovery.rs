//! Dependency discovery: which plugins the workspace's dependencies bring
//! within reach, and which of them the user has consented to.
//!
//! Discovery is the read side of the enablement axis. It runs in two phases:
//!
//! 1. list the workspace's dependencies ([`pm::workspace_dep_ids`]);
//! 2. ask each *untrusted* instance — the cargo transport — for the plugins
//!    its dependencies embed ([`PackageManager::active_plugins`]).
//!
//! Each such offer is then classified against the `[plugins]` config:
//! already enabled, auto-enabled, declined, or a candidate still awaiting
//! consent. Discovery itself neither fetches nor writes.
//!
//! On top of that read side sits the consent write side:
//! [`prompt_for_consent`] asks about each candidate and [`apply_consent`]
//! records the answers. The prompt is inert unless its [`Output`] is
//! interactive, so hook dispatch — and anything else an agent triggers —
//! can never block on stdin; those contexts get [`pending_candidates`] as a
//! `SessionStart` hint instead.
//!
//! Enablement matters because a dependency is deliberately *not* a trust
//! root: depending on a crate means compiling its code, not letting its
//! author inject agent context. Registry instances are trust roots — a
//! registry exists to curate plugins — but their plugins are loaded and
//! gated directly ([`plugins::load_registry`], evaluated by
//! [`Plugin::applies`]), so they never reach discovery. What discovery
//! classifies is exactly the untrusted offers: the dependency-embedded
//! plugins an ecosystem transport surfaces, which run only with consent.
//!
//! [`plugins::load_registry`]: crate::plugins::load_registry
//! [`Plugin::applies`]: crate::plugins::ParsedPlugin::applies
//!
//! [`pm::workspace_dep_ids`]: crate::pm::workspace_dep_ids
//! [`PackageManager::active_plugins`]: crate::pm::PackageManager::active_plugins

use std::path::Path;

use crate::pm::WorkspaceDeps;
use anyhow::{Context, Result};
use std::sync::Arc;

use crate::config::Symposium;
use crate::crate_sources::normalize_crate_name;
use crate::output::Output;
use crate::pm::{CARGO_PM, PackageId};
use crate::report::ReportEvent;

/// Why a discovered offer is (or is not) enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enablement {
    /// Enabled by a `[plugins] use` entry naming it.
    Used,
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
        matches!(self, Self::Used | Self::AutoEnabled)
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
    /// A short human summary of what the plugin contributes, for the consent
    /// prompt.
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
/// recorded for one workspace only). `active_plugins` fetches each dependency
/// cache-only — for a workspace dependency, into the source `cargo metadata`
/// already extracted (no probe, no network) — so every dependency-embedded
/// plugin is discoverable, registry crates included.
pub async fn discover(sym: &Symposium, deps: &Arc<WorkspaceDeps>) -> Discovery {
    let Some(workspace_root) = deps.workspace_root().map(Path::to_path_buf) else {
        return Discovery::default();
    };
    let pms = sym.package_managers(deps);
    let dep_ids = pms.list_deps().await.unwrap_or_default();

    let mut discovery = Discovery::default();
    // Untrusted instances = the cargo transport: its `active_plugins` are the
    // plugins embedded in dependencies, which run only with consent. Classify
    // each against the `[plugins]` config.
    for inst in pms.instances().filter(|i| !i.trusted) {
        for plugin in inst.active_plugins(&dep_ids).await {
            let name = plugin.canonical.name.clone();
            let description = Some(describe_plugin(&plugin.plugin));
            let enablement = decide(sym, &name, &workspace_root);
            let discovered = DiscoveredPlugin {
                registry: inst.name.clone(),
                id: plugin.canonical,
                recommends: name,
                description,
                enablement,
            };
            match enablement {
                Enablement::Used => discovery.active.push(discovered),
                Enablement::AutoEnabled => discovery.auto_enabled.push(discovered),
                Enablement::Declined => discovery.declined.push(discovered),
                Enablement::Candidate => discovery.candidates.push(discovered),
            }
        }
    }
    discovery
}

/// The crate names whose embedded plugins the user has enabled, to load at
/// sync time. Two sources:
///
/// 1. workspace **dependencies** covered by `[plugins] auto-enable` or an
///    applicable `use` entry, and
/// 2. crates named by a `use` entry that are **not** dependencies —
///    `cargo agents use <crate>` pulls a plugin in from its registry
///    (crates.io) whether or not the workspace depends on it.
///
/// `auto-enable` intentionally contributes only (1): it is consent for what a
/// dependency you already have carries, not a way to add crates. Declined
/// names are pruned. This reads the config rather than the offer list, so a
/// `use`d crate that isn't a dependency at all (source not resolved yet) still
/// works.
pub fn enabled_dependencies(
    sym: &Symposium,
    dep_ids: &[PackageId],
    workspace_root: &Path,
) -> Vec<String> {
    let plugins = &sym.config.plugins;
    let mut names: Vec<String> = dep_ids
        .iter()
        .filter(|id| id.pm == CARGO_PM)
        .filter(|id| !plugins.is_disabled(&id.name))
        .filter(|id| {
            plugins.is_auto_enabled(&id.name) || plugins.is_used_in(&id.name, workspace_root)
        })
        .map(|id| id.name.clone())
        .collect();

    for used in plugins.used_names_in(workspace_root) {
        let norm = normalize_crate_name(used);
        let known = plugins.is_disabled(used)
            || names.iter().any(|n| normalize_crate_name(n) == norm)
            || dep_ids
                .iter()
                .any(|id| normalize_crate_name(&id.name) == norm);
        if !known {
            names.push(used.to_string());
        }
    }

    names
}

/// Run [`discover`] for the workspace `deps` points at, or an empty
/// [`Discovery`] when there is no workspace.
pub async fn discover_for(sym: &Symposium, deps: &Arc<WorkspaceDeps>) -> Discovery {
    discover(sym, deps).await
}

/// The names of the discovered offers still awaiting consent, deduplicated
/// and sorted — what a consent prompt would ask about, and what the
/// non-interactive hint names.
pub async fn pending_candidates(sym: &Symposium, deps: &Arc<WorkspaceDeps>) -> Vec<String> {
    let mut names: Vec<String> = discover_for(sym, deps)
        .await
        .candidates
        .into_iter()
        .map(|c| c.id.name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Record consent decisions: approvals into `[plugins] auto-enable`,
/// declines into `[plugins] disable`, then save the config.
///
/// Split out from the prompt so the decision recording is testable without a
/// terminal, and so other entry points can record the same way.
pub fn apply_consent(sym: &mut Symposium, approved: &[String], declined: &[String]) -> Result<()> {
    if approved.is_empty() && declined.is_empty() {
        return Ok(());
    }
    let plugins = &mut sym.config.plugins;
    for name in approved {
        if !plugins.is_auto_enabled(name) {
            plugins.auto_enable.push(name.clone());
        }
    }
    for name in declined {
        if !plugins.is_disabled(name) {
            plugins.disable.push(name.clone());
        }
    }
    sym.save_config().context("failed to write user config")?;

    if !approved.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("enabled dependency plugins: {}", approved.join(", ")),
            },
        );
    }
    if !declined.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "declined dependency plugins (recorded in `[plugins] disable`): {}",
                    declined.join(", ")
                ),
            },
        );
    }
    Ok(())
}

/// Ask the user about each undecided offer, then record the answers.
///
/// **Never prompts unless `out` is interactive** ([`Output::is_interactive`]):
/// hook dispatch and anything an agent triggers use a quiet or capturing
/// output, so they return here immediately without touching stdin. In those
/// contexts the candidates surface as a `SessionStart` hint instead (see
/// [`pending_candidates`]).
///
/// Only explicit answers are recorded — the default ("ask me later") leaves
/// the dependency undecided, so reflexively hitting Enter never permanently
/// declines anything, and Escape leaves the remaining offers undecided too.
pub async fn prompt_for_consent(
    sym: &mut Symposium,
    deps: &Arc<WorkspaceDeps>,
    out: &Output,
) -> Result<()> {
    if !out.is_interactive() {
        return Ok(());
    }
    let candidates = discover_for(sym, deps).await.candidates;
    if candidates.is_empty() {
        return Ok(());
    }

    let mut approved = Vec::new();
    let mut declined = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        let name = candidate.id.name;
        if !seen.insert(name.clone()) {
            continue;
        }
        let what = candidate
            .description
            .as_deref()
            .unwrap_or("agent extensions");
        let answer = dialoguer::Select::new()
            .with_prompt(format!("Dependency `{name}` provides {what}. Enable it?"))
            .items(["Ask me later", "Enable", "No — don't ask again"])
            .default(0)
            .interact_opt()
            .context("consent prompt failed")?;
        match answer {
            Some(1) => approved.push(name),
            Some(2) => declined.push(name),
            Some(_) => {}  // ask me later — record nothing
            None => break, // Esc — leave the rest undecided
        }
    }
    apply_consent(sym, &approved, &declined)
}

/// A short human summary of what a discovered plugin contributes, for the
/// consent prompt and status output. Emphasizes the facets that matter to a
/// trust decision — a plugin that only ships skills is lower-stakes than one
/// that runs a hook or an MCP server.
fn describe_plugin(plugin: &crate::plugins::Plugin) -> String {
    let parts: Vec<String> = [
        count_phrase(plugin.skills.len(), "skill group", "skill groups"),
        count_phrase(plugin.hooks.len(), "hook", "hooks"),
        count_phrase(plugin.mcp_servers.len(), "MCP server", "MCP servers"),
        count_phrase(plugin.subcommands.len(), "subcommand", "subcommands"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        "agent extensions".to_string()
    } else {
        parts.join(", ")
    }
}

/// `"<n> <singular|plural>"`, or `None` when `n` is zero.
fn count_phrase(n: usize, singular: &str, plural: &str) -> Option<String> {
    (n > 0).then(|| format!("{n} {}", if n == 1 { singular } else { plural }))
}

/// Classify one offer against the `[plugins]` config. An explicit decision —
/// `use`, then `disable` — outranks the standing `auto-enable`, so a name the
/// user declined stays declined.
fn decide(sym: &Symposium, name: &str, workspace_root: &Path) -> Enablement {
    let plugins = &sym.config.plugins;
    // `disable` is tested first because it is the last word: every other
    // entry says a plugin *may* run, and this is the only one that says it
    // may not. Activation agrees (see `record_active`), so classifying a
    // disabled plugin as `Used` here would report it active while it stays
    // off.
    if plugins.is_disabled(name) {
        Enablement::Declined
    } else if plugins.is_used_in(name, workspace_root) {
        Enablement::Used
    } else if plugins.is_auto_enabled(name) {
        Enablement::AutoEnabled
    } else {
        Enablement::Candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::ANY_VERSION;
    use crate::pm::WorkspaceCrate;
    use indoc::indoc;

    /// A workspace with `widget-lib` as a path dependency carrying skills,
    /// plus a plain registry dependency (an extracted source with no plugin
    /// content, as `cargo metadata` always yields a `source_dir`).
    fn workspace(root: &Path) -> Arc<WorkspaceDeps> {
        let widget = root.join("widget-lib");
        std::fs::create_dir_all(widget.join("skills/guidance")).unwrap();
        std::fs::write(widget.join("skills/guidance/SKILL.md"), "").unwrap();
        let serde = root.join("serde-src");
        std::fs::create_dir_all(&serde).unwrap();
        WorkspaceDeps::fixture(
            root.to_path_buf(),
            vec![
                WorkspaceCrate::new(
                    "widget-lib".to_string(),
                    semver::Version::new(1, 0, 0),
                    Some(widget),
                ),
                WorkspaceCrate::new("serde".to_string(), semver::Version::new(1, 0, 210), None)
                    .with_source_dir(Some(serde)),
            ],
        )
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
        let ws = workspace(tmp.path());
        let sym = sym_with(tmp.path(), "");

        let found = discover(&sym, &ws).await;
        assert!(found.active.is_empty());
        assert!(found.auto_enabled.is_empty());
        let names: Vec<&str> = found.candidates.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert_eq!(found.candidates[0].recommends, "widget-lib");
    }

    #[tokio::test]
    async fn auto_enable_moves_a_candidate_to_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["widget_lib"]
            "#},
        );

        let found = discover(&sym, &ws).await;
        assert!(found.candidates.is_empty());
        let names: Vec<&str> = found.auto_enabled.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert!(found.enabled().count() == 1);
    }

    #[tokio::test]
    async fn use_entry_and_disable_outrank_the_standing_decisions() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());

        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["widget-lib"]
            "#},
        );
        let found = discover(&sym, &ws).await;
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
        let found = discover(&sym, &ws).await;
        assert!(found.auto_enabled.is_empty());
        assert_eq!(found.declined.len(), 1);
    }

    /// A `use` entry recorded for another workspace does not enable anything
    /// here.
    #[tokio::test]
    async fn workspace_scoped_use_entries_only_count_in_their_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = [{ name = "widget-lib", workspace = "/elsewhere" }]
            "#},
        );

        let found = discover(&sym, &ws).await;
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
            // A registry dependency, invisible to `active_plugins`, is still
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
            vec!["serde".to_string(), "tokio".to_string()]
        );
    }

    /// `use`-ing a crate that isn't a workspace dependency still enables it, so
    /// sync loads its plugin from the registry. `auto-enable` does not — it is
    /// consent for dependencies you already have.
    #[test]
    fn use_enables_a_non_dependency_crate_but_auto_enable_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["not-a-dep-autoenable"]
                use = ["my-skills-crate", { name = "scoped-crate", workspace = "/elsewhere" }]
            "#},
        );
        // Only `anyhow` is an actual dependency; the rest are not.
        let deps = [PackageId::new(CARGO_PM, "anyhow", "1.0.0")];

        let enabled = enabled_dependencies(&sym, &deps, tmp.path());
        // The `use`d non-dependency crate is enabled; the workspace-scoped one
        // (for /elsewhere) and the auto-enabled non-dependency are not.
        assert_eq!(enabled, vec!["my-skills-crate".to_string()]);
    }

    /// A `use`d crate that *is* a dependency appears once, not twice.
    #[test]
    fn used_dependency_is_not_duplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["serde"]
            "#},
        );
        let deps = [PackageId::new(CARGO_PM, "serde", "1.0.210")];
        assert_eq!(
            enabled_dependencies(&sym, &deps, tmp.path()),
            vec!["serde".to_string()]
        );
    }
}
