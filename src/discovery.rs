//! Dependency discovery: which plugins the workspace's dependencies bring
//! within reach, and which of them the user has consented to.
//!
//! Discovery is the read side of the enablement axis. It runs in two phases:
//!
//! 1. list the workspace's dependencies ([`Workspace::dep_ids`]);
//! 2. ask each *untrusted* instance — the ecosystem transports — for the plugins
//!    its dependencies embed ([`PackageManager::active_plugins`]).
//!
//! Each such offer is then classified against the `[plugins]` config and the
//! [audits list](crate::audits): already enabled,
//! auto-enabled, declined, revoked, or a candidate still awaiting consent.
//! Each offer not revoked also carries what the audits say about it (a
//! [`Verdict`]), which the prompt, the hint and `status` show; only a
//! revocation changes the classification. Discovery itself neither fetches nor writes.
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
//! [`Plugin::applies`]: crate::plugins::Plugin::applies
//!
//! [`Workspace::dep_ids`]: crate::pm::Workspace::dep_ids
//! [`PackageManager::active_plugins`]: crate::pm::PackageManager::active_plugins

use std::collections::HashSet;
use std::path::Path;

use crate::pm::Workspace;
use anyhow::{Context, Result};
use std::sync::Arc;
use symposium_install::UpdateLevel;

use crate::audits::{Answer, AuditIndex, Revocation, Verdict};
use crate::config::Symposium;
use crate::output::Output;
use crate::plugins::Plugin;
use crate::pm::{PackageId, PmRegistry};
use crate::report::ReportEvent;

/// Why a discovered offer is (or is not) enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enablement {
    /// Enabled by a `[plugins] use` entry naming it.
    Used,
    /// Enabled ahead of time by `[plugins] auto-enable`.
    AutoEnabled,
    /// Declined: `[plugins] disable` names it.
    Declined,
    /// The audits list revokes this release or payload. Outranks
    /// `use` and `auto-enable`; only `disable` outranks it.
    Revoked(Revocation),
    /// Nobody has decided yet — this is what a consent prompt would ask about.
    Candidate,
}

impl Enablement {
    /// Does this decision let the plugin run?
    pub fn is_enabled(&self) -> bool {
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
    pub format: Option<&'static str>,
    /// What the audits list says about this release and payload, looked up
    /// once, cache-only. `None` exactly when the list revokes it: the
    /// revocation is the `enablement` of a revoked offer, and a declined one
    /// stays declined.
    pub audit: Option<Verdict>,
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
    /// Revoked by the audits list. Never loaded, whatever else
    /// says it may be, and never offered for consent.
    pub revoked: Vec<DiscoveredPlugin>,
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
pub async fn discover(sym: &Symposium, ws: &Arc<Workspace>, registry: &[Plugin]) -> Discovery {
    let Some(workspace_root) = ws.root().await.map(Path::to_path_buf) else {
        return Discovery::default();
    };
    let pms = ws.pms();
    let audits = AuditIndex::configured(sym);
    let dep_ids = ws.dep_ids().await;
    let used: HashSet<PackageId> =
        resolve_used(sym, pms, registry, &workspace_root, UpdateLevel::None)
            .await
            .into_iter()
            .map(|(_, id)| id)
            .collect();

    let mut discovery = Discovery::default();
    // Untrusted instances are ecosystem transports: their `active_plugins`
    // are embedded in dependencies and run only with consent. Classify
    // each against the `[plugins]` config.
    for inst in pms.instances().filter(|i| !i.trusted) {
        for plugin in inst.active_plugins(dep_ids).await {
            let name = plugin.canonical.name.clone();
            let description = Some(match plugin.format() {
                Some(_) => format!(
                    "the Agent Plugins package `{}` ({})",
                    plugin.manifest.name,
                    describe_plugin(&plugin.manifest)
                ),
                None => describe_plugin(&plugin.manifest),
            });
            let (revocation, audit) = match audits.answer(pms, &plugin.canonical).await {
                Answer::Revoked(revocation) => (Some(revocation), None),
                Answer::Verdict(verdict) => (None, Some(verdict)),
            };
            let enablement = decide(sym, pms, revocation, &used, &plugin.canonical).await;
            let discovered = DiscoveredPlugin {
                registry: inst.name.clone(),
                format: plugin.format(),
                id: plugin.canonical,
                recommends: name,
                description,
                enablement,
                audit,
            };
            match discovered.enablement {
                Enablement::Used => discovery.active.push(discovered),
                Enablement::AutoEnabled => discovery.auto_enabled.push(discovered),
                Enablement::Declined => discovery.declined.push(discovered),
                Enablement::Revoked(_) => discovery.revoked.push(discovered),
                Enablement::Candidate => discovery.candidates.push(discovered),
            }
        }
    }
    discovery
}

/// The packages whose plugins the user has enabled, to load at sync time.
/// Two sources:
///
/// 1. workspace **dependencies** covered by `[plugins] auto-enable` or an
///    applicable `use` entry, and
/// 2. packages named by a `use` entry that are **not** dependencies:
///    `cargo agents use <crate>` pulls a plugin in from crates.io whether or
///    not the workspace depends on it.
///
/// `auto-enable` intentionally contributes only (1): it is consent for what a
/// dependency you already have carries, not a way to add crates. Declined
/// packages are pruned. A `use` name is resolved through the package managers
/// ([`PmRegistry::identify`]) at `update`: one that names a plugin in
/// `registry` is left out, since that plugin is loaded already and
/// [`used_plugins`] wakes it, and one that no package manager has is skipped.
pub async fn enabled_dependencies(
    sym: &Symposium,
    pms: &PmRegistry,
    registry: &[Plugin],
    dep_ids: &[PackageId],
    workspace_root: &Path,
    update: UpdateLevel,
) -> Vec<PackageId> {
    let plugins = &sym.config.plugins;
    let used = resolve_used(sym, pms, registry, workspace_root, update).await;
    let mut enabled = Vec::new();
    let mut known = HashSet::new();
    for id in dep_ids {
        let Ok(identity) = pms.identity(id).await else {
            continue;
        };
        let is_used = used.iter().any(|(_, used)| *used == identity);
        known.insert(identity);
        if !plugins.is_disabled(pms, id).await
            && (plugins.is_auto_enabled(pms, id).await || is_used)
        {
            enabled.push(PackageId::any_version(&id.pm, &id.name));
        }
    }

    let mut registry_ids = HashSet::new();
    for plugin in registry {
        if let Ok(id) = pms.identity(&plugin.canonical).await {
            registry_ids.insert(id);
        }
    }
    for (name, id) in used {
        let named = PackageId::any_version(&id.pm, name);
        if !registry_ids.contains(&id)
            && !plugins.is_disabled(pms, &named).await
            && known.insert(id)
        {
            enabled.push(named);
        }
    }

    enabled
}

pub(crate) async fn resolve_used<'a>(
    sym: &'a Symposium,
    pms: &PmRegistry,
    registry: &[Plugin],
    workspace_root: &Path,
    update: UpdateLevel,
) -> Vec<(&'a str, PackageId)> {
    let mut used = Vec::new();
    for name in sym.config.plugins.used_names_in(workspace_root) {
        match pms.identify(registry, name, update).await {
            Ok(id) => used.push((name, id)),
            Err(e) => tracing::warn!(error = %e, "skipping `[plugins] use` entry"),
        }
    }
    used
}

/// The canonical ids of the `plugins` an applicable `use` entry names, which
/// is what wakes a dormant one. A plugin is named by its manifest name, under
/// the rule of the package manager it came from.
pub async fn used_plugins(
    sym: &Symposium,
    pms: &PmRegistry,
    plugins: &[Plugin],
    workspace_root: &Path,
) -> Vec<PackageId> {
    let mut used = Vec::new();
    for plugin in plugins {
        let named = PackageId::any_version(&plugin.canonical.pm, &plugin.manifest.name);
        if sym
            .config
            .plugins
            .is_used_in(pms, &named, workspace_root)
            .await
        {
            used.push(plugin.canonical.clone());
        }
    }
    used
}

/// Run [`discover`] for the workspace `deps` points at, against the plugins
/// its registries and members define, or an empty [`Discovery`] when there is
/// no workspace.
pub async fn discover_for(sym: &Symposium, ws: &Arc<Workspace>) -> Discovery {
    let workspace = ws.info().await.cloned();
    let registry = crate::plugins::load_registry_with_workspace(sym, workspace.as_ref()).await;
    discover(sym, ws, &registry.plugins).await
}

/// The discovered offers still awaiting consent, one per name, sorted by
/// name — what a consent prompt would ask about, and what the
/// non-interactive hint names, each with its audit answer.
pub async fn pending_candidates(sym: &Symposium, deps: &Arc<Workspace>) -> Vec<DiscoveredPlugin> {
    let mut candidates = discover_for(sym, deps).await.candidates;
    candidates.sort_by(|a, b| a.id.name.cmp(&b.id.name));
    candidates.dedup_by(|a, b| a.id.name == b.id.name);
    candidates
}

/// Record consent decisions: approvals into `[plugins] auto-enable`,
/// declines into `[plugins] disable`, then save the config. Decisions retain
/// the discovered package's owner so deduplication uses that PM's name rules;
/// the config stores the package name.
///
/// Split out from the prompt so the decision recording is testable without a
/// terminal, and so other entry points can record the same way.
pub async fn apply_consent(
    sym: &mut Symposium,
    pms: &PmRegistry,
    approved: &[PackageId],
    declined: &[PackageId],
) -> Result<()> {
    if approved.is_empty() && declined.is_empty() {
        return Ok(());
    }
    let plugins = &mut sym.config.plugins;
    for id in approved {
        if !crate::config::names_match(pms, plugins.auto_enable.iter().map(String::as_str), id)
            .await?
        {
            plugins.auto_enable.push(id.name.clone());
        }
    }
    for id in declined {
        if !crate::config::names_match(pms, plugins.disable.iter().map(String::as_str), id).await? {
            plugins.disable.push(id.name.clone());
        }
    }
    sym.save_config().context("failed to write user config")?;

    if !approved.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("enabled dependency plugins: {}", approved.iter().map(|id| id.name.as_str()).collect::<Vec<_>>().join(", ")),
            },
        );
    }
    if !declined.is_empty() {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!(
                    "declined dependency plugins (recorded in `[plugins] disable`): {}",
                    declined.iter().map(|id| id.name.as_str()).collect::<Vec<_>>().join(", ")
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
/// Each question also says what the audits say about the plugin; that
/// changes nothing about what is asked or recorded.
pub async fn prompt_for_consent(
    sym: &mut Symposium,
    deps: &Arc<Workspace>,
    out: &Output,
) -> Result<()> {
    if !out.is_interactive() {
        return Ok(());
    }
    let candidates = pending_candidates(sym, deps).await;
    if candidates.is_empty() {
        return Ok(());
    }

    let mut approved = Vec::new();
    let mut declined = Vec::new();
    for candidate in candidates {
        let answer = dialoguer::Select::new()
            .with_prompt(consent_question(&candidate))
            .items(["Ask me later", "Enable", "No — don't ask again"])
            .default(0)
            .interact_opt()
            .context("consent prompt failed")?;
        match answer {
            Some(1) => approved.push(candidate.id),
            Some(2) => declined.push(candidate.id),
            Some(_) => {}  // ask me later — record nothing
            None => break, // Esc — leave the rest undecided
        }
    }
    apply_consent(sym, deps.pms(), &approved, &declined).await
}

/// The question the consent prompt asks about one candidate: what it
/// provides and what the audits say about it, in the words `status` uses. A
/// candidate is never revoked, so it always has a verdict; like a `status`
/// line, the question would simply leave it out otherwise.
fn consent_question(candidate: &DiscoveredPlugin) -> String {
    let what = candidate
        .description
        .as_deref()
        .unwrap_or("agent extensions");
    let audit = candidate
        .audit
        .as_ref()
        .map(|audit| format!(" — {audit}"))
        .unwrap_or_default();
    format!(
        "Dependency `{}` provides {what}{audit}. Enable it?",
        candidate.name()
    )
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

/// Classify one offer against the `[plugins]` config and the `revocation`
/// the audits list holds for it, if any. Precedence, first match wins:
/// `disable` (the last word), then a revocation (outranks every grant, so a
/// revoked release stays out whatever consented to it), then `use`, then the
/// standing `auto-enable`; anything else is a candidate. An audit is not a
/// grant: an audited offer nobody consented to is a candidate like any other.
async fn decide(
    sym: &Symposium,
    pms: &PmRegistry,
    revocation: Option<Revocation>,
    used: &HashSet<PackageId>,
    id: &PackageId,
) -> Enablement {
    let plugins = &sym.config.plugins;
    // `disable` is tested first because it is the last word: every other
    // entry says a plugin *may* run, and this is the only one that says it
    // may not. Activation agrees (see `record_active`), so classifying a
    // disabled plugin as `Used` here would report it active while it stays
    // off.
    let Ok(identity) = pms.identity(id).await else {
        return Enablement::Declined;
    };
    if plugins.is_disabled(pms, id).await {
        Enablement::Declined
    } else if let Some(revocation) = revocation {
        Enablement::Revoked(revocation)
    } else if used.contains(&identity) {
        Enablement::Used
    } else if plugins.is_auto_enabled(pms, id).await {
        Enablement::AutoEnabled
    } else {
        Enablement::Candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::WorkspaceCrate;
    use crate::pm::{ANY_VERSION, CARGO_PM};
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

        let found = discover(&sym, &ws, &[]).await;
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

        let found = discover(&sym, &ws, &[]).await;
        assert!(found.candidates.is_empty());
        let names: Vec<&str> = found.auto_enabled.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert!(found.enabled().count() == 1);
    }

    /// [`sym_with`], with the audits list on and `widget-lib`'s audits file
    /// holding `audits` (no file when empty).
    fn sym_with_audits(root: &Path, config: &str, audits: &str) -> Symposium {
        let mut sym = sym_with(root, config);
        sym.config.defaults.symposium_audits = true;
        let dir = sym.audits_dir().unwrap().join("audits/cargo");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("widget-lib.toml");
        if audits.is_empty() {
            let _ = std::fs::remove_file(file);
        } else {
            std::fs::write(file, audits).unwrap();
        }
        sym
    }

    /// An audit of `widget-lib`'s payload in the [`workspace`] fixture.
    fn widget_audited(root: &Path) -> String {
        let digest = crate::audits::digest::payload_digest(&root.join("widget-lib")).unwrap();
        format!("format = 1\n[[audit]]\ndigest = \"{digest}\"\n")
    }

    /// A revocation outranks every grant (`use`, `auto-enable`) and is
    /// outranked only by `disable`. A revoked offer is never a candidate, and
    /// has no verdict to show in place of the revocation.
    #[tokio::test]
    async fn revocation_outranks_grants_and_only_disable_outranks_it() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let revoke = "format = 1\n[[revoked]]\nversions = \"^1\"\nreason = \"exfiltration\"\n";
        let revoked = Enablement::Revoked(Revocation {
            reason: symposium_sdk::audits::RevokeReason::Exfiltration,
        });
        for config in [
            "",
            "[plugins]\nuse = [\"widget-lib\"]",
            "[plugins]\nauto-enable = [\"widget-lib\"]",
        ] {
            let found = discover(&sym_with_audits(tmp.path(), config, revoke), &ws, &[]).await;
            assert!(found.candidates.is_empty(), "{config}");
            assert_eq!(found.enabled().count(), 0, "{config}");
            assert_eq!(found.revoked.len(), 1, "{config}");
            assert_eq!(found.revoked[0].enablement, revoked, "{config}");
            assert_eq!(found.revoked[0].audit, None, "{config}");
        }
        assert!(!revoked.is_enabled());

        let sym = sym_with_audits(
            tmp.path(),
            "[plugins]\nuse = [\"widget-lib\"]\ndisable = [\"widget-lib\"]",
            revoke,
        );
        let found = discover(&sym, &ws, &[]).await;
        assert!(found.revoked.is_empty());
        assert_eq!(found.declined.len(), 1);
        // Declined, and still revoked: no verdict to show in its place.
        assert_eq!(found.declined[0].audit, None);
        assert!(
            pending_candidates(&sym_with_audits(tmp.path(), "", revoke), &ws)
                .await
                .is_empty()
        );
    }

    /// An audit changes nothing about consent in this release: an audited
    /// offer is still a candidate, and the consent prompt still asks.
    #[tokio::test]
    async fn an_audited_offer_is_still_a_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let sym = sym_with_audits(tmp.path(), "", &widget_audited(tmp.path()));
        let found = discover(&sym, &ws, &[]).await;
        let names: Vec<&str> = found.candidates.iter().map(|c| c.name()).collect();
        assert_eq!(names, vec!["widget-lib"]);
        assert_eq!(found.candidates[0].audit, Some(Verdict::Audited));
    }

    /// A candidate offering one skill group, with the given verdict.
    fn candidate_with(name: &str, audit: Verdict) -> DiscoveredPlugin {
        DiscoveredPlugin {
            registry: CARGO_PM.to_string(),
            id: PackageId::new(CARGO_PM, name, "1.0.0"),
            recommends: name.to_string(),
            description: Some("1 skill group".to_string()),
            enablement: Enablement::Candidate,
            format: None,
            audit: Some(audit),
        }
    }

    /// Each question says what the audits say, in the words `status` uses,
    /// and for a payload that cannot be digested, why in a few words.
    #[test]
    fn the_consent_question_says_whether_the_plugin_is_audited() {
        let questions: Vec<String> = [
            candidate_with("audited-lib", Verdict::Audited),
            candidate_with("plain-lib", Verdict::NotAudited),
            candidate_with(
                "git-lib",
                Verdict::Ineligible(crate::audits::digest::Ineligible::GitSkillSource(
                    "https://github.com/example/skills".into(),
                )),
            ),
        ]
        .iter()
        .map(consent_question)
        .collect();
        assert_eq!(
            questions,
            [
                "Dependency `audited-lib` provides 1 skill group — audited by the `symposium-audits` list. Enable it?",
                "Dependency `plain-lib` provides 1 skill group — not audited. Enable it?",
                "Dependency `git-lib` provides 1 skill group — not audited: its payload cannot be checked against an audit because it fetches skills with `source.git`. Enable it?",
            ]
        );
    }

    /// The list audits the package, but this payload has no digest: the
    /// candidate is not audited, and its answer says why.
    #[tokio::test]
    async fn an_undigestable_audited_offer_says_why_it_is_not_audited() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace(tmp.path());
        let audited = widget_audited(tmp.path());
        std::fs::write(
            tmp.path().join("widget-lib/Symposium.toml"),
            "[[skills]]\nsource.git = \"https://github.com/example/skills\"\n",
        )
        .unwrap();

        let pending = pending_candidates(&sym_with_audits(tmp.path(), "", &audited), &ws).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].audit,
            Some(Verdict::Ineligible(
                crate::audits::digest::Ineligible::GitSkillSource(
                    "https://github.com/example/skills".into()
                )
            ))
        );

        // Nobody audited a package with no audits file: no reason is given,
        // since a digest would not have changed the answer.
        let pending = pending_candidates(&sym_with_audits(tmp.path(), "", ""), &ws).await;
        assert_eq!(pending[0].audit, Some(Verdict::NotAudited));
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
        let found = discover(&sym, &ws, &[]).await;
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
        let found = discover(&sym, &ws, &[]).await;
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

        let found = discover(&sym, &ws, &registry.plugins).await;
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

        let found = discover(&sym, &ws, &[]).await;
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

    async fn enabled_names(sym: &Symposium, deps: &[PackageId], root: &Path) -> Vec<String> {
        enabled_dependencies(sym, &cached_crates(), &[], deps, root, UpdateLevel::None)
            .await
            .into_iter()
            .map(|id| id.name)
            .collect()
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
            // The same name in another ecosystem is independently enabled.
            PackageId::new("npm", "serde", ANY_VERSION),
        ];

        assert_eq!(
            enabled_dependencies(
                &sym,
                &cached_crates(),
                &[],
                &deps,
                tmp.path(),
                UpdateLevel::None,
            )
            .await,
            vec![
                PackageId::any_version(CARGO_PM, "serde"),
                PackageId::any_version(CARGO_PM, "tokio"),
                PackageId::any_version("npm", "serde"),
            ]
        );
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

        // The `use`d non-dependency crate is enabled; the workspace-scoped one
        // (for /elsewhere), the unknown one and the auto-enabled
        // non-dependency are not.
        assert_eq!(
            enabled_names(&sym, &deps, tmp.path()).await,
            vec!["my-skills-crate".to_string()]
        );
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
        assert_eq!(
            enabled_names(&sym, &deps, tmp.path()).await,
            vec!["serde".to_string()]
        );
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
            used_plugins(&sym, &sym.detached_managers(), &plugins, tmp.path()).await
                == [plugins[0].canonical.clone()]
        };

        assert!(used("my-plugin").await);
        assert!(!used("my_plugin").await);
    }
}
