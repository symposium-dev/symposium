//! Dependency discovery: which plugins the workspace's dependencies bring
//! within reach, and which of them the user has consented to.
//!
//! Discovery is the read side of the enablement axis. It runs in two phases:
//!
//! 1. list the workspace's dependencies ([`Workspace::dep_ids`]);
//! 2. ask each *untrusted* instance (the ecosystem transports: cargo, and any
//!    `[[package-manager]]`) for the plugins its dependencies embed
//!    ([`PackageManager::active_plugins`]).
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
//! [`Plugin::applies`]: Plugin::applies
//!
//! [`Workspace::dep_ids`]: crate::pm::Workspace::dep_ids
//! [`PackageManager::active_plugins`]: crate::pm::PackageManager::active_plugins

use std::path::Path;

use crate::pm::Workspace;
use anyhow::{Context, Result};
use std::sync::Arc;
use symposium_install::UpdateLevel;

use crate::config::{PluginRef, Spellings, Symposium};
use crate::output::Output;
use crate::plugins::Plugin;
use crate::pm::{PackageId, PmRegistry};
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
    /// The offering package-manager instance: the ecosystem transport whose
    /// dependency embeds the plugin.
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
    /// Already enabled: named by a `use` entry.
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

/// What a bare `[plugins] use` name means in one workspace.
///
/// A bare name names the one package manager it can mean, looking nearest
/// first:
///
/// 1. a registry plugin of that name;
/// 2. a dependency of that name;
/// 3. an ecosystem transport, which can load a package that is no dependency.
///
/// The first of these with a match answers, so a recommendation and a crate
/// on crates.io sharing a name is not an ambiguity. Matches from more than one
/// package manager there are, and the entry enables nothing until it names its
/// `pm`, rather than enabling a plugin the user may not have meant. A registry
/// plugin answers whether or not it is dormant: a bare name that once woke it
/// must not start loading a crate of that name when the plugin gains a gate.
///
/// Names compare under the
/// [`canonical_name`](crate::pm::PackageManager::canonical_name) of the
/// package manager each candidate belongs to.
pub struct Names<'a> {
    registry: Vec<PackageId>,
    deps: Vec<PackageId>,
    pms: &'a PmRegistry,
}

impl<'a> Names<'a> {
    pub fn new(registry: &[Plugin], deps: &[PackageId], pms: &'a PmRegistry) -> Self {
        Self {
            registry: registry.iter().map(|p| p.canonical.clone()).collect(),
            deps: deps.to_vec(),
            pms,
        }
    }

    /// Whether `name` names `id`, as spelled or under `id`'s package
    /// manager's canonical form. Only a name spelled another way asks it.
    async fn same(&self, id: &PackageId, name: &str) -> bool {
        id.name == name
            || self.pms.canonical_name(&id.pm, &id.name).await
                == self.pms.canonical_name(&id.pm, name).await
    }

    /// The packages `name` could mean from the nearest of the registry
    /// plugins and the dependencies that has one, one per package. `pm`
    /// narrows both. Empty when neither has it.
    pub async fn nearest(&self, name: &str, pm: Option<&str>) -> Vec<PackageId> {
        let found = self.named(&self.registry, name, pm).await;
        if found.is_empty() {
            self.named(&self.deps, name, pm).await
        } else {
            found
        }
    }

    async fn named(&self, ids: &[PackageId], name: &str, pm: Option<&str>) -> Vec<PackageId> {
        let mut found: Vec<PackageId> = Vec::new();
        for id in ids {
            let seen = found.iter().any(|f| f.pm == id.pm);
            if !seen && pm.is_none_or(|pm| id.pm == pm) && self.same(id, name).await {
                found.push(id.clone());
            }
        }
        found
    }

    /// The registry plugins and dependencies each `auto-enable` and `disable`
    /// entry names here under another spelling than its own.
    pub async fn spellings(&self, plugins: &crate::config::PluginsConfig) -> Spellings {
        let mut spellings = Spellings::default();
        for id in self.registry.iter().chain(&self.deps) {
            self.spell(plugins, id, &mut spellings).await;
        }
        spellings
    }

    /// Add to `spellings` the `auto-enable` and `disable` entries that name
    /// `id` under another spelling than its own: for a package loaded after
    /// [`spellings`](Self::spellings), which only knows the registry plugins
    /// and dependencies.
    pub async fn spell(
        &self,
        plugins: &crate::config::PluginsConfig,
        id: &PackageId,
        spellings: &mut Spellings,
    ) {
        for entry in plugins.auto_enable.iter().chain(&plugins.disable) {
            if entry.name != "*"
                && entry.name != id.name
                && entry.pm.as_ref().is_none_or(|pm| *pm == id.pm)
                && self.same(id, &entry.name).await
            {
                spellings.add(entry.clone(), id.clone());
            }
        }
    }

    /// `entry` naming its package manager, spelled the way that package
    /// manager reports the package when it is known here; `None` for a bare
    /// name that no registry plugin or dependency has, which only an
    /// ecosystem package manager can answer for ([`enabled_dependencies`]).
    /// A bare name that could mean more than one is an error listing the
    /// package managers it could mean. A wildcard is left bare: it names every
    /// package manager's packages.
    pub async fn qualify(&self, entry: &PluginRef) -> Result<Option<PluginRef>, Vec<String>> {
        if entry.name == "*" {
            return Ok(Some(entry.clone()));
        }
        let found = self.nearest(&entry.name, entry.pm.as_deref()).await;
        if let Some(pm) = &entry.pm {
            return Ok(Some(match found.first() {
                Some(id) => PluginRef::new(pm, &id.name),
                None => entry.clone(),
            }));
        }
        let mut pms: Vec<String> = found.iter().map(|id| id.pm.clone()).collect();
        pms.sort();
        pms.dedup();
        match (pms.as_slice(), found.first()) {
            ([pm], Some(id)) => Ok(Some(PluginRef::new(pm, &id.name))),
            ([], _) => Ok(None),
            _ => Err(pms),
        }
    }
}

/// The `use` entries that apply in one workspace, resolved by [`Names`].
#[derive(Debug, Default)]
pub struct Used {
    /// What they enable, each naming its package manager.
    pub plugins: Vec<PluginRef>,
    /// Bare names no registry plugin or dependency has, left for the ecosystem
    /// package managers to say whether they have one
    /// ([`enabled_dependencies`]).
    pub unknown: Vec<PluginRef>,
    /// Bare names that enable nothing, with the package managers each could
    /// mean, so the user can be told to pick one.
    pub ambiguous: Vec<(PluginRef, Vec<String>)>,
}

/// Resolve the `use` entries that apply in `workspace_root`.
pub async fn used(sym: &Symposium, workspace_root: &Path, names: &Names<'_>) -> Used {
    let mut used = Used::default();
    for entry in sym.config.plugins.used_in(workspace_root) {
        match names.qualify(entry).await {
            Ok(Some(plugin)) => used.plugins.push(plugin),
            Ok(None) => used.unknown.push(entry.clone()),
            Err(pms) => {
                tracing::debug!(entry = %entry, ?pms, "ambiguous `use` entry enables nothing");
                used.ambiguous.push((entry.clone(), pms));
            }
        }
    }
    used
}

/// What the `use` entries enable in the workspace at `workspace_root`, or
/// nothing outside one: [`used`], for the callers building a predicate
/// context.
pub async fn used_plugins(
    sym: &Symposium,
    workspace_root: Option<&Path>,
    registry: &[Plugin],
    deps: &[PackageId],
    pms: &PmRegistry,
) -> Vec<PluginRef> {
    match workspace_root {
        Some(root) => {
            used(sym, root, &Names::new(registry, deps, pms))
                .await
                .plugins
        }
        None => Vec::new(),
    }
}

/// Discover the plugins offered for this workspace's dependencies.
///
/// `used` is what the applicable `use` entries enable ([`used`]), and
/// `spellings` what the other entries name here ([`Names::spellings`]). Each
/// untrusted package manager answers from its own dependencies' sources (the
/// cargo PM reads the sources `cargo metadata` already extracted), so every
/// dependency-embedded plugin is discoverable, registry packages included.
pub async fn discover(
    sym: &Symposium,
    ws: &Arc<Workspace>,
    used: &[PluginRef],
    spellings: &Spellings,
) -> Discovery {
    if ws.root().await.is_none() {
        return Discovery::default();
    }
    let pms = ws.pms();
    let dep_ids = ws.dep_ids().await;

    let mut discovery = Discovery::default();
    // Untrusted instances are the ecosystem transports: their `active_plugins`
    // are the plugins embedded in dependencies, which run only with consent.
    // Classify each against the `[plugins]` config, under its own id.
    for inst in pms.instances().filter(|i| !i.trusted) {
        for plugin in inst.active_plugins(dep_ids).await {
            let name = plugin.canonical.name.clone();
            let description = Some(describe_plugin(&plugin.manifest));
            let enablement = decide(sym, &plugin.canonical, used, dep_ids, spellings);
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

/// The packages whose embedded plugins the user has enabled, to load at sync
/// time, from any package manager. Two sources:
///
/// 1. workspace **dependencies** covered by `[plugins] auto-enable` or an
///    applicable `use` entry, and
/// 2. packages named by a `use` entry that are **not** dependencies:
///    `cargo agents use <crate>` pulls a plugin in from its registry
///    (crates.io) whether or not the workspace depends on it.
///
/// `auto-enable` intentionally contributes only (1): it is consent for what a
/// dependency you already have carries, not a way to add packages. Declined
/// ones are pruned. This reads the config rather than the offer list, so a
/// `use`d package that isn't a dependency at all (source not resolved yet)
/// still works.
///
/// `used` is what the applicable `use` entries enable ([`used`]), and
/// `spellings` what the other entries name here ([`Names::spellings`]). An
/// entry counts here only when it names one of the package managers in `pms`
/// that is not a trust root. One naming a registry wakes a dormant registry plugin
/// where it already is, so there is nothing to load. A bare name no registry
/// plugin or dependency has is looked for in each ecosystem package manager
/// ([`PmInstance::find`](crate::pm::PmInstance::find)) at `update`: under
/// [`UpdateLevel::None`] only on disk, so a hook never searches; found by
/// exactly one, it is that package; by several, it is ambiguous and enables
/// nothing; by none, it is skipped.
pub async fn enabled_dependencies(
    sym: &Symposium,
    pms: &PmRegistry,
    dep_ids: &[PackageId],
    used: &Used,
    spellings: &Spellings,
    update: UpdateLevel,
) -> Vec<PackageId> {
    let plugins = &sym.config.plugins;
    let found = find_unknown(pms, &used.unknown, update).await;
    let used: Vec<&PluginRef> = used.plugins.iter().chain(&found).collect();
    let mut ids: Vec<PackageId> = dep_ids
        .iter()
        .filter(|id| !plugins.is_disabled(id, spellings))
        .filter(|id| {
            plugins.is_auto_enabled(id, dep_ids, spellings) || used.iter().any(|u| u.matches(id))
        })
        .map(|id| PackageId::any_version(&id.pm, &id.name))
        .collect();

    for used in used {
        // A wildcard names dependencies, which the first pass covered.
        let Some(pm) = used.pm.as_deref().filter(|_| used.name != "*") else {
            continue;
        };
        if !pms.instances().any(|i| !i.trusted && i.name == pm) {
            continue;
        }
        let id = PackageId::any_version(pm, &used.name);
        let known =
            plugins.is_disabled(&id, spellings) || ids.iter().any(|known| used.matches(known));
        if !known {
            ids.push(id);
        }
    }

    ids
}

/// The packages the ecosystem package managers have under each of `unknown`'s
/// bare names, for those exactly one has.
pub(crate) async fn find_unknown(
    pms: &PmRegistry,
    unknown: &[PluginRef],
    update: UpdateLevel,
) -> Vec<PluginRef> {
    let mut found = Vec::new();
    for entry in unknown {
        let mut have = Vec::new();
        for inst in pms.instances() {
            if let Some(id) = inst.find(&entry.name, update).await {
                have.push(id);
            }
        }
        match have.as_slice() {
            [id] => found.push(PluginRef::new(&id.pm, &id.name)),
            [] => tracing::debug!(entry = %entry, "no package manager has this `use` entry"),
            _ => tracing::warn!(
                entry = %entry,
                pms = ?have.iter().map(|id| &id.pm).collect::<Vec<_>>(),
                "ambiguous `use` entry enables nothing; name its package manager"
            ),
        }
    }
    found
}

/// Run [`discover`] for the workspace `deps` points at, against the plugins
/// its registries and members define, or an empty [`Discovery`] when there is
/// no workspace.
pub async fn discover_for(sym: &Symposium, ws: &Arc<Workspace>) -> Discovery {
    let Some(workspace) = ws.info().await.cloned() else {
        return Discovery::default();
    };
    let registry = crate::plugins::load_registry_with_workspace(sym, Some(&workspace)).await;
    let names = Names::new(&registry.plugins, ws.dep_ids().await, ws.pms());
    let used = used(sym, &workspace.root, &names).await.plugins;
    let spellings = names.spellings(&sym.config.plugins).await;
    discover(sym, ws, &used, &spellings).await
}

/// The discovered offers still awaiting consent, deduplicated and sorted —
/// what a consent prompt would ask about, and what the non-interactive hint
/// names.
pub async fn pending_candidates(sym: &Symposium, deps: &Arc<Workspace>) -> Vec<PluginRef> {
    let mut pending: Vec<PluginRef> = discover_for(sym, deps)
        .await
        .candidates
        .into_iter()
        .map(|c| PluginRef::new(&c.id.pm, &c.id.name))
        .collect();
    pending.sort_by_key(ToString::to_string);
    pending.dedup();
    pending
}

/// Record consent decisions: approvals into `[plugins] auto-enable`,
/// declines into `[plugins] disable`, then save the config. Both lists hold
/// dependency crate names, as [`discover`] offers them.
///
/// Split out from the prompt so the decision recording is testable without a
/// terminal, and so other entry points can record the same way.
///
/// Each decision names the package manager that offered the plugin as well as
/// its name, so consent to one ecosystem's package never covers another's of
/// the same name.
pub fn apply_consent(
    sym: &mut Symposium,
    approved: &[PackageId],
    declined: &[PackageId],
) -> Result<()> {
    if approved.is_empty() && declined.is_empty() {
        return Ok(());
    }
    let plugins = &mut sym.config.plugins;
    for id in approved {
        let entry = PluginRef::new(&id.pm, &id.name);
        if !plugins.auto_enable.contains(&entry) {
            plugins.auto_enable.push(entry);
        }
    }
    for id in declined {
        let entry = PluginRef::new(&id.pm, &id.name);
        if !plugins.disable.contains(&entry) {
            plugins.disable.push(entry);
        }
    }
    sym.save_config().context("failed to write user config")?;

    if !approved.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("enabled dependency plugins: {}", shown(approved)),
            },
        );
    }
    if !declined.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "declined dependency plugins (recorded in `[plugins] disable`): {}",
                    shown(declined)
                ),
            },
        );
    }
    Ok(())
}

/// `ids` as the user is shown them: `pm:name`.
fn shown(ids: &[PackageId]) -> String {
    ids.iter()
        .map(|id| PluginRef::new(&id.pm, &id.name).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `cargo agents use` command that enables `plugin`, which names its
/// package manager: by name and `--pm`, since `use` takes the name a package
/// manager knows a package by, not the `pm:name` it is shown as.
pub fn use_command(plugin: &PluginRef) -> String {
    match &plugin.pm {
        Some(pm) => format!("cargo agents use {} --pm {pm}", plugin.name),
        None => format!("cargo agents use {}", plugin.name),
    }
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
    deps: &Arc<Workspace>,
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
        let id = PackageId::any_version(&candidate.id.pm, &candidate.id.name);
        if !seen.insert(id.clone()) {
            continue;
        }
        let name = PluginRef::new(&id.pm, &id.name);
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
            Some(1) => approved.push(id),
            Some(2) => declined.push(id),
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
fn describe_plugin(plugin: &crate::plugins::PluginManifest) -> String {
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

/// Classify one offer against the `[plugins]` config: `disable`, then `use`,
/// then the standing `auto-enable`, so a name the user declined stays
/// declined.
fn decide(
    sym: &Symposium,
    id: &PackageId,
    used: &[PluginRef],
    deps: &[PackageId],
    spellings: &Spellings,
) -> Enablement {
    let plugins = &sym.config.plugins;
    // `disable` is tested first because it is the last word: every other
    // entry says a plugin *may* run, and this is the only one that says it
    // may not. Activation agrees (see `record_active`), so classifying a
    // disabled plugin as `Used` here would report it active while it stays
    // off.
    if plugins.is_disabled(id, spellings) {
        Enablement::Declined
    } else if used.iter().any(|entry| entry.matches(id)) {
        Enablement::Used
    } else if plugins.is_auto_enabled(id, deps, spellings) {
        Enablement::AutoEnabled
    } else {
        Enablement::Candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::WorkspaceCrate;
    use crate::pm::{ANY_VERSION, CARGO_PM, PmRegistry};
    use indoc::indoc;

    /// A workspace with `widget-lib` as a path dependency carrying skills,
    /// plus a plain registry dependency (an extracted source with no plugin
    /// content, as `cargo metadata` always yields a `source_dir`).
    fn workspace(root: &Path) -> Arc<Workspace> {
        let widget = root.join("widget-lib");
        std::fs::create_dir_all(widget.join("skills/guidance")).unwrap();
        std::fs::write(widget.join("skills/guidance/SKILL.md"), "").unwrap();
        let serde = root.join("serde-src");
        std::fs::create_dir_all(&serde).unwrap();
        Workspace::fixture(
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

        let found = discover_for(&sym, &ws).await;
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

        let found = discover_for(&sym, &ws).await;
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
        let found = discover_for(&sym, &ws).await;
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
        let found = discover_for(&sym, &ws).await;
        assert!(found.auto_enabled.is_empty());
        assert_eq!(found.declined.len(), 1);
    }

    #[tokio::test]
    async fn use_naming_a_registry_plugin_leaves_the_dependency_undecided() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["widget-lib"]

                [[registry]]
                name = "reg"
                path = "reg"
            "#},
        );
        let entry = tmp.path().join("config/reg/widget-lib");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("Symposium.toml"), "name = \"widget-lib\"\n").unwrap();
        let registry = crate::plugins::load_registry(&sym).await;
        let names = Names::new(&registry.plugins, ws.dep_ids().await, ws.pms());
        let used = used(&sym, tmp.path(), &names).await.plugins;
        let spellings = names.spellings(&sym.config.plugins).await;

        let found = discover(&sym, &ws, &used, &spellings).await;
        assert!(found.active.is_empty());
        assert_eq!(found.candidates.len(), 1);
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

        let found = discover_for(&sym, &ws).await;
        assert!(found.active.is_empty());
        assert_eq!(found.candidates.len(), 1);
    }

    /// The crates the tests below name, all cached: every dependency they
    /// list, plus `my-skills-crate`, which no workspace depends on.
    fn cached_crates() -> PmRegistry {
        let (cargo, _) = crate::test_utils::FakeCargo::instance(
            &["serde", "tokio", "clap", "anyhow", "my-skills-crate"],
            &[],
        );
        PmRegistry::new(vec![cargo])
    }

    /// [`enabled_dependencies`] with the `use` entries resolved the way the
    /// active set resolves them, with no registry plugins, against `pms`.
    async fn enabled_with(
        sym: &Symposium,
        pms: &PmRegistry,
        deps: &[PackageId],
        root: &Path,
    ) -> Vec<PackageId> {
        let names = Names::new(&[], deps, pms);
        let used = used(sym, root, &names).await;
        let spellings = names.spellings(&sym.config.plugins).await;
        enabled_dependencies(sym, pms, deps, &used, &spellings, UpdateLevel::None).await
    }

    /// [`enabled_with`] against [`cached_crates`].
    async fn enabled(sym: &Symposium, deps: &[PackageId], root: &Path) -> Vec<PackageId> {
        enabled_with(sym, &cached_crates(), deps, root).await
    }

    /// Hook dispatch, `--help` and subcommand dispatch build the active set at
    /// `UpdateLevel::None`, so a `use` entry for a crate that is neither a
    /// dependency nor cached is skipped without a crates.io search or a cargo
    /// process; `sync` resolves it at `Check`.
    #[tokio::test]
    async fn resolving_use_entries_without_update_stays_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["published-crate"]
            "#},
        );
        let registry = crate::plugins::load_registry(&sym).await;
        let (cargo, network) = crate::test_utils::FakeCargo::instance(&[], &["published-crate"]);
        let pms = PmRegistry::new(vec![cargo]);
        let active_at = async |update| {
            let mut ctx = crate::predicate::PredicateContext::new(&[]);
            crate::plugins::active_plugins(
                &sym,
                &registry,
                &pms,
                Some(tmp.path()),
                &mut ctx,
                update,
            )
            .await
        };

        active_at(UpdateLevel::None).await;
        assert_eq!(network.load(std::sync::atomic::Ordering::SeqCst), 0);

        active_at(UpdateLevel::Check).await;
        assert_eq!(network.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn enabled_dependencies_reads_config_not_offers() {
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
        ];
        assert_eq!(
            enabled(&sym, &deps, tmp.path()).await,
            cargo(&["serde", "tokio"])
        );

        // A bare name two package managers' dependencies share consents to
        // neither.
        let deps = [
            deps.as_slice(),
            &[PackageId::new("npm", "serde", ANY_VERSION)],
        ]
        .concat();
        assert_eq!(enabled(&sym, &deps, tmp.path()).await, cargo(&["tokio"]));
    }

    fn registry_plugin(pm: &str, name: &str, dormant: bool) -> crate::plugins::Plugin {
        crate::plugins::Plugin {
            canonical: PackageId::any_version(pm, name),
            manifest: crate::plugins::PluginManifest {
                name: name.to_string(),
                requires_use: dormant,
                predicates: Default::default(),
                hooks: vec![],
                skills: vec![],
                mcp_servers: vec![],
                installations: vec![],
                subcommands: Default::default(),
                custom_predicates: vec![],
                chained: vec![],
            },
            workspace_member: false,
        }
    }

    /// An `auto-enable` or `disable` entry names a package spelled another way
    /// when its package manager reads the two as one: cargo's `widget-lib` for
    /// `widget_lib`, whether a dependency or a package loaded later. A package
    /// manager that compares names exactly does not.
    #[tokio::test]
    async fn an_entry_names_a_package_its_package_manager_spells_another_way() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["widget_lib"]
                disable = ["noisy_crate", "my_plugin", "chained_crate"]
            "#},
        );
        let registry = [registry_plugin("recs", "my-plugin", false)];
        let deps = [
            PackageId::new(CARGO_PM, "widget-lib", "1.0.0"),
            PackageId::new(CARGO_PM, "noisy-crate", "1.0.0"),
        ];
        let pms = sym.detached_managers();
        let names = Names::new(&registry, &deps, &pms);
        let plugins = &sym.config.plugins;
        let mut spellings = names.spellings(plugins).await;

        assert!(plugins.is_auto_enabled(&deps[0], &deps, &spellings));
        assert!(plugins.is_disabled(&deps[1], &spellings));
        assert!(!plugins.is_disabled(&registry[0].canonical, &spellings));

        let chained = PackageId::new(CARGO_PM, "chained-crate", "1.0.0");
        assert!(!plugins.is_disabled(&chained, &spellings));
        names.spell(plugins, &chained, &mut spellings).await;
        assert!(plugins.is_disabled(&chained, &spellings));
        // Without asking, only the spelling written matches.
        let exact = crate::config::Spellings::default();
        assert!(!plugins.is_auto_enabled(&deps[0], &deps, &exact));
        assert!(!plugins.is_disabled(&deps[1], &exact));
    }

    /// A bare name means the one package manager it can, nearest first: a
    /// registry plugin, then a dependency. One neither has is for the
    /// ecosystem package managers to find.
    #[tokio::test]
    async fn a_bare_name_resolves_nearest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let npm = "\n[[package-manager]]\nname = \"npm\"\ncommand = \"/nonexistent/pm\"\n";
        let cargo_only = sym_with(tmp.path(), "");
        let with_npm = sym_with(tmp.path(), npm);
        let registry = [
            registry_plugin("recs", "rtk", true),
            registry_plugin("recs", "serde", false),
            registry_plugin("recs", "twice", true),
            registry_plugin("more-recs", "twice", true),
        ];
        let deps = [
            PackageId::new(CARGO_PM, "serde", "1.0.0"),
            PackageId::new(CARGO_PM, "rtk", "1.0.0"),
            PackageId::new(CARGO_PM, "tokio", "1.0.0"),
            PackageId::new(CARGO_PM, "shared", "1.0.0"),
            PackageId::new("npm", "shared", "1.0.0"),
        ];
        let qualify = async |sym: &Symposium, name: &str| {
            let pms = sym.detached_managers();
            Names::new(&registry, &deps, &pms)
                .qualify(&PluginRef::bare(name))
                .await
        };

        // A recommendation outranks the crate of its name, dormant or not, so
        // a bare name that woke one never starts loading the crate instead.
        assert_eq!(
            qualify(&cargo_only, "rtk").await,
            Ok(Some(PluginRef::new("recs", "rtk")))
        );
        assert_eq!(
            qualify(&cargo_only, "serde").await,
            Ok(Some(PluginRef::new("recs", "serde")))
        );
        assert_eq!(
            qualify(&cargo_only, "tokio").await,
            Ok(Some(PluginRef::new(CARGO_PM, "tokio")))
        );
        // A name no registry plugin or dependency has is left for the
        // ecosystem package managers to say whether they have it.
        assert_eq!(qualify(&cargo_only, "anyhow").await, Ok(None));
        // Two matches in the nearest tier are ambiguous.
        assert_eq!(
            qualify(&cargo_only, "twice").await,
            Err(vec!["more-recs".to_string(), "recs".to_string()])
        );
        assert_eq!(
            qualify(&cargo_only, "shared").await,
            Err(vec![CARGO_PM.to_string(), "npm".to_string()])
        );
        assert_eq!(qualify(&with_npm, "anyhow").await, Ok(None));
        // A qualified entry and a wildcard are left as they are.
        let pms = cargo_only.detached_managers();
        let names = Names::new(&registry, &deps, &pms);
        assert_eq!(
            names.qualify(&PluginRef::new("npm", "shared")).await,
            Ok(Some(PluginRef::new("npm", "shared")))
        );
        assert_eq!(
            names.qualify(&PluginRef::bare("*")).await,
            Ok(Some(PluginRef::bare("*")))
        );
    }

    fn cargo(names: &[&str]) -> Vec<PackageId> {
        names
            .iter()
            .map(|n| PackageId::any_version(CARGO_PM, *n))
            .collect()
    }

    /// Enablement is per package manager: an entry naming `npm` enables that
    /// ecosystem's package, provided an `npm` package manager is configured to
    /// load it.
    #[tokio::test]
    async fn enablement_reaches_every_package_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let config = indoc! {r#"
            [plugins]
            auto-enable = [{ pm = "npm", name = "left-pad" }]
            use = [{ pm = "npm", name = "is-even" }, { pm = "nowhere", name = "x" }]
        "#};
        let deps = [
            PackageId::new("npm", "left-pad", "1.3.0"),
            PackageId::new("npm", "is-odd", "3.0.1"),
        ];

        let sym = sym_with(
            tmp.path(),
            &format!(
                "{config}\n[[package-manager]]\nname = \"npm\"\ncommand = \"/nonexistent/symposium-pm-npm\"\n"
            ),
        );
        assert_eq!(
            enabled_with(&sym, &sym.detached_managers(), &deps, tmp.path()).await,
            vec![
                PackageId::any_version("npm", "left-pad"),
                PackageId::any_version("npm", "is-even"),
            ]
        );

        // With no `npm` package manager, nothing could load `is-even`.
        let sym = sym_with(tmp.path(), config);
        assert_eq!(
            enabled_with(&sym, &sym.detached_managers(), &deps, tmp.path()).await,
            vec![PackageId::any_version("npm", "left-pad")]
        );
    }

    /// Consent is recorded precisely, under the package manager that offered
    /// the plugin, so it never covers another ecosystem's package of the same
    /// name, here or in a workspace that depends on that one.
    #[test]
    fn consent_is_recorded_under_the_offering_package_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sym = sym_with(tmp.path(), "");
        apply_consent(
            &mut sym,
            &[
                PackageId::any_version("npm", "left-pad"),
                PackageId::any_version(CARGO_PM, "widget"),
            ],
            &[PackageId::any_version("npm", "is-odd")],
        )
        .unwrap();

        let plugins = &sym.config.plugins;
        assert_eq!(
            plugins.auto_enable,
            vec![
                PluginRef::new("npm", "left-pad"),
                PluginRef::new(CARGO_PM, "widget")
            ]
        );
        assert_eq!(plugins.disable, vec![PluginRef::new("npm", "is-odd")]);
        let cargo_left_pad = PackageId::any_version(CARGO_PM, "left-pad");
        assert!(!plugins.is_auto_enabled(
            &cargo_left_pad,
            std::slice::from_ref(&cargo_left_pad),
            &crate::config::Spellings::default()
        ));
    }

    /// `use`-ing a crate that isn't a workspace dependency still enables it, so
    /// sync loads its plugin from the registry. `auto-enable` does not: it is
    /// consent for dependencies you already have. A `use` name no package
    /// manager has is skipped.
    #[tokio::test]
    async fn use_enables_a_non_dependency_crate_but_auto_enable_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                auto-enable = ["not-a-dep-autoenable"]
                use = ["my-skills-crate", "ghost", { name = "scoped-crate", workspace = "/elsewhere" }]
            "#},
        );
        // Only `anyhow` is an actual dependency; the rest are not.
        let deps = [PackageId::new(CARGO_PM, "anyhow", "1.0.0")];

        let enabled = enabled(&sym, &deps, tmp.path()).await;
        // The `use`d non-dependency crate is enabled; the workspace-scoped one
        // (for /elsewhere) and the auto-enabled non-dependency are not.
        assert_eq!(enabled, cargo(&["my-skills-crate"]));
    }

    /// A `use`d crate that *is* a dependency appears once, not twice.
    #[tokio::test]
    async fn used_dependency_is_not_duplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = sym_with(
            tmp.path(),
            indoc! {r#"
                [plugins]
                use = ["serde"]
            "#},
        );
        let deps = [PackageId::new(CARGO_PM, "serde", "1.0.210")];
        assert_eq!(enabled(&sym, &deps, tmp.path()).await, cargo(&["serde"]));
    }

    /// A registry compares plugin names exactly, so only the exact spelling
    /// in `use` wakes a dormant registry plugin.
    #[tokio::test]
    async fn use_wakes_a_registry_plugin_only_by_its_exact_name() {
        let used = async |entry: &str| {
            let tmp = tempfile::tempdir().unwrap();
            let config = format!(
                "[plugins]\nuse = [\"{entry}\"]\n\n[[registry]]\nname = \"reg\"\npath = \"reg\"\n"
            );
            let sym = sym_with(tmp.path(), &config);
            let entry_dir = tmp.path().join("config/reg/my-plugin");
            std::fs::create_dir_all(&entry_dir).unwrap();
            std::fs::write(entry_dir.join("Symposium.toml"), "name = \"my-plugin\"\n").unwrap();
            let plugins = crate::plugins::load_registry(&sym).await.plugins;
            assert_eq!(plugins.len(), 1);
            assert!(plugins[0].manifest.requires_use);
            let used = used_plugins(
                &sym,
                Some(tmp.path()),
                &plugins,
                &[],
                &sym.detached_managers(),
            )
            .await;
            crate::predicate::PredicateContext::new(&[])
                .with_used(&used)
                .is_used(&plugins[0].canonical)
        };

        assert!(used("my-plugin").await);
        assert!(!used("my_plugin").await);
    }
}
