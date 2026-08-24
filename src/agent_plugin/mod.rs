//! Compiling a gated symposium plugin into an agent plugin directory.
//!
//! The directory is the unit agents themselves use: a manifest beside a
//! `skills/` folder. Compilation happens after every predicate has been
//! evaluated, so what lands on disk is only what applies — an agent never
//! receives a gate and never resolves one.

pub mod manifest;
pub mod read;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::PluginsConfig;
use crate::plugins::ParsedPlugin;
use crate::pm::{ANY_VERSION, CARGO_PM};
use crate::skills::SkillWithGroupContext;
use manifest::{GeminiExtension, Manifest, Marketplace, MarketplaceEntry};

/// The directory under a project root that symposium owns outright, so one
/// `.gitignore` at its root covers everything below it.
pub const PROJECT_OWNED_DIR: &str = ".symposium";

/// Staging directory for compiled plugins within [`PROJECT_OWNED_DIR`].
pub const PROJECT_STAGING_SUBDIR: &str = "plugins";

/// Marketplace name for the global staging root.
const MARKETPLACE_NAME: &str = "symposium";

/// Marketplace name for a staging root.
///
/// A project root needs a name of its own because marketplace *registration* is
/// user-level even for a project-scoped plugin (verified against Claude Code), so
/// two projects both registering `symposium` would overwrite each other's path.
pub fn marketplace_name(scope: Scope, project_root: Option<&Path>) -> String {
    match (scope, project_root) {
        (Scope::Project, Some(root)) => {
            let scoped = format!("{MARKETPLACE_NAME}-{}", crate::pm::workspace_dir_name(root));
            manifest::slug(&scoped).unwrap_or_else(|| MARKETPLACE_NAME.to_string())
        }
        _ => MARKETPLACE_NAME.to_string(),
    }
}

/// Staging directory under the user configuration directory.
///
/// Deliberately not `plugins/`, which is the builtin `user-plugins` *registry* —
/// a directory symposium reads entries from. Compiling into it would make
/// symposium ingest its own output as registry plugins on the next load.
pub const GLOBAL_STAGING_DIR: &str = "installed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Project,
    Global,
}

impl Scope {
    /// Where a plugin's compiled directory belongs. Global needs both a
    /// `use --global` entry naming it and every gate in its chain — the plugin's,
    /// each declared group's, each contributed skill's — to hold
    /// workspace-independently; anything else is project-scoped.
    ///
    /// The second half is correctness, not preference: a user-level directory is
    /// visible everywhere while cleanup reaps what it did not install this run,
    /// so a global set that varied by workspace would have two projects undoing
    /// each other every session. Content counts as much as activation, hence the
    /// group and skill gates.
    pub fn of(
        parsed: &ParsedPlugin,
        contributed: &[&SkillWithGroupContext],
        plugins: &PluginsConfig,
    ) -> Scope {
        let workspace_bound = parsed.workspace_member
            || parsed.canonical.pm == CARGO_PM
            || !parsed.plugin.predicates.is_workspace_independent()
            || parsed
                .plugin
                .skills
                .iter()
                .any(|group| !group.predicates.is_workspace_independent())
            || contributed
                .iter()
                .any(|entry| !entry.skill.predicates.is_workspace_independent());
        if workspace_bound || !plugins.is_used_globally(&parsed.plugin.name) {
            Scope::Project
        } else {
            Scope::Global
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::Global => "global",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSkill {
    pub dir_name: String,
    /// Directory holding the skill's `SKILL.md`, copied verbatim.
    pub source_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPlugin {
    /// The plugin this was compiled from. Lets a caller tell whether a given
    /// skill is already covered by a delivered plugin directory.
    pub source_id: crate::pm::PackageId,
    pub dir_name: String,
    pub manifest: Manifest,
    pub scope: Scope,
    pub skills: Vec<CompiledSkill>,
}

/// Group already-gated skills into one compiled plugin per contributing plugin.
///
/// A plugin with no applicable skills compiles to nothing: version one carries
/// only the format's skills component, so such a directory would be empty.
pub fn compile(
    active: &[ParsedPlugin],
    skills: &[SkillWithGroupContext],
    plugins: &PluginsConfig,
) -> Vec<CompiledPlugin> {
    let mut compiled: Vec<(String, CompiledPlugin)> = Vec::new();
    // One skill bundle referenced by several plugins is emitted once, by the
    // first plugin to claim it. Emitting it per plugin would load identical
    // guidance N times, since a plugin directory is its own namespace.
    let mut claimed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for parsed in active {
        let mine: Vec<&SkillWithGroupContext> = skills
            .iter()
            .filter(|s| s.plugin_id == parsed.canonical)
            .collect();
        if mine.is_empty() {
            continue;
        }

        let Some(name) = manifest::slug(&parsed.plugin.name) else {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!(
                        "cannot compile plugin `{}`: no valid agent plugin name",
                        parsed.plugin.name
                    ),
                },
            );
            continue;
        };

        // Every skill this plugin declares may already have been claimed by an
        // earlier one, so emptiness is only known after dedup.
        let skills = compile_skills(&mine, &mut claimed);
        if skills.is_empty() {
            continue;
        }

        compiled.push((
            crate::skills::hash_origin_key(&parsed.canonical.to_string()),
            CompiledPlugin {
                source_id: parsed.canonical.clone(),
                dir_name: name.clone(),
                manifest: Manifest::new(
                    name,
                    version_of(parsed),
                    parsed.plugin.description.clone(),
                ),
                scope: Scope::of(parsed, &mine, plugins),
                skills,
            },
        ));
    }

    disambiguate(compiled)
}

/// Two plugin names can slug to the same directory name, so whenever more than
/// one plugin claims a slug, every claimant takes the suffixed form. Suffixing
/// all of them rather than all-but-one keeps a name stable when an unrelated
/// plugin appears or disappears.
///
/// The manifest name is suffixed alongside the directory: agents key a plugin
/// by its manifest name, so leaving that colliding would give two plugins one
/// enablement entry and one cache path.
fn disambiguate(compiled: Vec<(String, CompiledPlugin)>) -> Vec<CompiledPlugin> {
    let mut claims: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, plugin) in &compiled {
        *claims.entry(plugin.dir_name.as_str()).or_default() += 1;
    }
    let contested: std::collections::BTreeSet<String> = claims
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(name, _)| name.to_string())
        .collect();

    compiled
        .into_iter()
        .map(|(hash, mut plugin)| {
            if contested.contains(&plugin.dir_name) {
                let name = manifest::suffixed(&plugin.dir_name, &hash);
                plugin.dir_name = name.clone();
                plugin.manifest.name = name;
            }
            plugin
        })
        .collect()
}

/// One skill directory per distinct origin, skipping origins an earlier plugin
/// already claimed. Skills sharing a name within one plugin take the origin-hash
/// suffix; across plugins names cannot collide, because the agent namespaces a
/// plugin's skills under the plugin.
fn compile_skills(
    skills: &[&SkillWithGroupContext],
    claimed: &mut std::collections::BTreeSet<String>,
) -> Vec<CompiledSkill> {
    let mut name_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    let mut distinct: Vec<&&SkillWithGroupContext> = Vec::new();

    for skill in skills {
        if claimed.insert(skill.origin_hash.clone()) {
            *name_counts.entry(skill.skill.name()).or_default() += 1;
            distinct.push(skill);
        }
    }

    distinct
        .into_iter()
        .filter_map(|entry| {
            let name = entry.skill.name();
            let source_dir = entry.skill.path.parent()?.to_path_buf();
            let dir_name = if name_counts.get(name).copied().unwrap_or(0) == 1 {
                name.to_string()
            } else {
                format!("{name}-{}", entry.origin_hash)
            };
            Some(CompiledSkill {
                dir_name,
                source_dir,
            })
        })
        .collect()
}

/// Stands in for a plugin that declares no version anywhere.
pub const UNVERSIONED: &str = "0.0.0";

/// The manifest's version wins; otherwise a crate plugin's resolved version
/// stands in. A registry or workspace plugin has no real package identity, so
/// its placeholder `*` is not a version.
fn version_of(parsed: &ParsedPlugin) -> String {
    parsed
        .plugin
        .version
        .clone()
        .or_else(|| {
            (parsed.canonical.version != ANY_VERSION).then(|| parsed.canonical.version.clone())
        })
        .unwrap_or_else(|| UNVERSIONED.to_string())
}

/// Write a compiled plugin into `root`, returning its directory.
///
/// The content is assembled in a temporary directory and then handed to the
/// ordinary managed-directory sync, so the install is change-aware and
/// debounced exactly like a skill directory: recompiling identical content
/// leaves the destination untouched.
pub fn write(
    compiled: &CompiledPlugin,
    root: &Path,
    boundary: &Path,
    debounce: Duration,
) -> Result<PathBuf> {
    let staged = tempfile::tempdir().context("create staging dir")?;
    write_manifests(staged.path(), &compiled.manifest)?;

    for skill in &compiled.skills {
        let dest = staged.path().join("skills").join(&skill.dir_name);
        fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
        crate::sync::copy_dir_recursive(&skill.source_dir, &dest)
            .with_context(|| format!("copy skill {}", skill.dir_name))?;
    }

    let dest = root.join(&compiled.dir_name);
    crate::sync::sync_managed_dir(
        staged.path(),
        &dest,
        boundary,
        debounce,
        crate::sync::Marking::MarkerOnly,
    )?;
    Ok(dest)
}

/// Every dialect of the same identity, side by side. Claude Code ignores a root
/// `plugin.json` and Agent Plugins agents ignore `.claude-plugin/`, so carrying
/// both costs nothing and saves a second directory; Gemini reads only its own
/// file. Verified by loading one directory in Claude Code, Codex, and Copilot.
fn write_manifests(dir: &Path, manifest: &Manifest) -> Result<()> {
    fs::write(dir.join("plugin.json"), manifest.to_json()).context("write plugin.json")?;

    let claude_dir = dir.join(".claude-plugin");
    fs::create_dir_all(&claude_dir).context("create .claude-plugin")?;
    fs::write(claude_dir.join("plugin.json"), manifest.to_json())
        .context("write .claude-plugin/plugin.json")?;

    let gemini = GeminiExtension::new(manifest.name.clone(), manifest.version.clone());
    fs::write(dir.join("gemini-extension.json"), gemini.to_json())
        .context("write gemini-extension.json")
}

/// Write the marketplace index for a staging root, or remove it when the root no
/// longer holds any compiled plugin. Claude Code, Codex, and Copilot all
/// discover plugins through this one file.
pub fn write_marketplace(root: &Path, name: &str, plugins: &[&CompiledPlugin]) -> Result<()> {
    let dir = root.join(".claude-plugin");
    let file = dir.join("marketplace.json");

    if plugins.is_empty() {
        if file.exists() {
            fs::remove_file(&file).with_context(|| format!("remove {}", file.display()))?;
        }
        return Ok(());
    }

    let entries = plugins
        .iter()
        .map(|plugin| MarketplaceEntry {
            name: plugin.manifest.name.clone(),
            source: format!("./{}", plugin.dir_name),
            description: plugin.manifest.description.clone(),
        })
        .collect();

    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let contents = Marketplace::new(name.to_string(), entries).to_json();
    if fs::read_to_string(&file).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    fs::write(&file, contents).with_context(|| format!("write {}", file.display()))
}

/// Reap marked directories under `root` that this sync did not write, descending
/// at most `depth` levels. Keyed on the ownership marker, so a directory the user
/// put there is left alone, and a marked directory is never descended into.
///
/// The depth is what lets one function serve both a staging root (plugins sit
/// directly under it) and an agent's own tree, where Codex nests its copies as
/// `<marketplace>/<plugin>/<version>`.
pub fn reap_to_depth(root: &Path, depth: usize, written: &std::collections::BTreeSet<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !crate::sync::has_symposium_marker(&path) {
            reap_to_depth(&path, depth - 1, written);
            continue;
        }
        if written.contains(&path) {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => tracing::info!(
                report = %crate::report::ReportEvent::SkillRemoved {
                    path: crate::output::display_path(&path),
                },
            ),
            Err(e) => tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!(
                        "failed to remove stale {}: {e}",
                        crate::output::display_path(&path)
                    ),
                },
            ),
        }
    }
}

/// Reap the plugins directly under a staging root.
pub fn reap(root: &Path, written: &std::collections::BTreeSet<PathBuf>) {
    reap_to_depth(root, 1, written)
}

/// How deep an agent nests its own plugin copies: Codex uses
/// `<marketplace>/<plugin>/<version>`, the others one or two levels.
pub const AGENT_COPY_DEPTH: usize = 3;

#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod tests;
