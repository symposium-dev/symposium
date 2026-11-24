use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use symposium_install::UpdateLevel;

use crate::agents::CompiledPlugin;
use crate::config::{PluginsConfig, Symposium, UseEntry};
use crate::plugins::{Plugin, PluginRegistry};
use crate::pm::{PackageId, PmRegistry};
use crate::predicate::PredicateContext;
use crate::report::ReportEvent;
use crate::skills::SkillWithGroupContext;
use crate::sync::{self, TreeContents};

const UNVERSIONED: &str = "0.0.0";
const AGENT_PLUGINS_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const MANIFEST_FILE: &str = "plugin.json";
const CLAUDE_MANIFEST_DIR: &str = ".claude-plugin";
const SKILLS_DIR: &str = "skills";
const MAX_NAME_LEN: usize = 64;
const PROJECT_DIR: &str = ".symposium";
const PROJECT_PLUGINS_SUBDIR: &str = "plugins";

async fn globally_used(
    config: &PluginsConfig,
    pms: &PmRegistry,
    plugins: &[Plugin],
) -> Vec<PackageId> {
    let mut used = Vec::new();
    for entry in &config.used {
        if let UseEntry::Global(name) = entry
            && let Ok(id) = pms.identify(plugins, name, UpdateLevel::None).await
        {
            used.push(id);
        }
    }
    used
}

async fn installs_globally(pms: &PmRegistry, used: &[PackageId], plugin: &Plugin) -> bool {
    !plugin.workspace_member
        && pms
            .identity(&plugin.canonical)
            .await
            .is_ok_and(|id| used.contains(&id))
}

pub(crate) async fn installed_for_user<'a>(
    sym: &Symposium,
    pms: &PmRegistry,
    registry_plugins: &[Plugin],
    plugins: impl IntoIterator<Item = &'a Plugin>,
) -> Vec<&'a Plugin> {
    let used = globally_used(&sym.config.plugins, pms, registry_plugins).await;
    let mut selected = Vec::new();
    for plugin in plugins {
        if installs_globally(pms, &used, plugin).await {
            selected.push(plugin);
        }
    }
    selected
}

pub(crate) fn is_plugin_dir(dir: &Path) -> bool {
    dir.join(CLAUDE_MANIFEST_DIR).join(MANIFEST_FILE).is_file()
        && !dir.join(crate::pm::layout::SKILL_FILE).exists()
}

pub(crate) fn project_plugins_root(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_DIR).join(PROJECT_PLUGINS_SUBDIR)
}

pub(crate) struct PluginSkills {
    name: String,
    version: String,
    skills: Vec<SkillWithGroupContext>,
}

fn is_version(version: &str) -> bool {
    semver::Version::parse(version).is_ok()
}

fn plugin_version(plugin: &Plugin) -> String {
    let declared = plugin.manifest.version.as_deref();
    let resolved = Some(plugin.canonical.version.as_str());
    [declared, resolved]
        .into_iter()
        .flatten()
        .find(|version| is_version(version))
        .unwrap_or(UNVERSIONED)
        .to_string()
}

pub(crate) fn compiled_version(plugin_dir: &Path) -> Result<String> {
    let path = plugin_dir.join(MANIFEST_FILE);
    let manifest = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest).with_context(|| format!("parse {}", path.display()))?;
    manifest["version"]
        .as_str()
        .filter(|version| is_version(version))
        .map(str::to_string)
        .with_context(|| format!("{} has no version", path.display()))
}

pub(crate) async fn global_plugin_set(
    sym: &Symposium,
    pms: &PmRegistry,
    registry: &PluginRegistry,
    active: &[Plugin],
    ctx: &mut PredicateContext<'_>,
) -> Vec<Plugin> {
    let inactive = registry
        .plugins
        .iter()
        .filter(|plugin| !active.iter().any(|a| a.canonical == plugin.canonical));
    let mut plugins: Vec<Plugin> =
        installed_for_user(sym, pms, &registry.plugins, active.iter().chain(inactive))
            .await
            .into_iter()
            .cloned()
            .collect();
    ctx.set_conditions_met(true);
    plugins.retain(|plugin| plugin.applies(ctx));
    ctx.set_conditions_met(false);
    plugins
}

pub(crate) async fn global_plugins(
    sym: &Symposium,
    pms: &PmRegistry,
    registry: &PluginRegistry,
    active: &[Plugin],
    ctx: &mut PredicateContext<'_>,
    update: UpdateLevel,
) -> Vec<PluginSkills> {
    let plugins = global_plugin_set(sym, pms, registry, active, ctx).await;
    ctx.set_conditions_met(true);
    let skills = crate::skills::collect_skills(sym, &plugins, ctx, update).await;
    ctx.set_conditions_met(false);
    group_skills(plugins, skills)
}

pub(crate) fn project_plugins(
    active: &[Plugin],
    applicable: &[SkillWithGroupContext],
    global: &[PluginSkills],
) -> Vec<PluginSkills> {
    let global_origins: BTreeSet<&str> = global
        .iter()
        .flat_map(|plugin| &plugin.skills)
        .map(|skill| skill.origin_hash.as_str())
        .collect();
    let skills = applicable
        .iter()
        .filter(|skill| !global_origins.contains(skill.origin_hash.as_str()))
        .cloned()
        .collect();
    group_skills(active.to_vec(), skills)
}

fn group_skills(plugins: Vec<Plugin>, skills: Vec<SkillWithGroupContext>) -> Vec<PluginSkills> {
    let mut grouped: Vec<PluginSkills> = plugins
        .into_iter()
        .map(|plugin| PluginSkills {
            version: plugin_version(&plugin),
            name: plugin.manifest.name,
            skills: Vec::new(),
        })
        .collect();
    let mut seen = BTreeSet::new();
    for skill in skills {
        if seen.insert((skill.skill.name().to_string(), skill.origin_hash.clone())) {
            grouped[skill.plugin_index].skills.push(skill);
        }
    }
    grouped.retain(|plugin| !plugin.skills.is_empty());
    grouped
}

#[derive(Default)]
pub(crate) struct Compiled {
    pub plugins: Vec<CompiledPlugin>,
    pub skill_origins: BTreeMap<String, Vec<String>>,
}

impl Compiled {
    pub(crate) fn skill_origins_of(&self, delivered: &[String]) -> BTreeSet<&String> {
        delivered
            .iter()
            .filter_map(|name| self.skill_origins.get(name))
            .flatten()
            .collect()
    }
}

pub(crate) fn compile(
    root: &Path,
    plugins: &[PluginSkills],
    debounce: Duration,
    warned: &mut BTreeSet<String>,
    skip_cleanup: bool,
) -> Compiled {
    let mut compiled = Compiled::default();
    for (plugin, name) in plugins.iter().zip(plugin_dir_names(plugins)) {
        let Some(name) = name else {
            warn(format!(
                "skipping plugin {:?}: its name has no ASCII letters or digits to name a plugin directory",
                plugin.name
            ));
            continue;
        };
        let dir = root.join(&name);
        let manifest = manifest(&name, &plugin.version);
        match write_plugin(&dir, &manifest, &skill_dirs(plugin), debounce, warned) {
            Ok(updated) => {
                let origins = plugin.skills.iter().map(|s| s.origin_hash.clone());
                compiled
                    .skill_origins
                    .insert(name.clone(), origins.collect());
                compiled.plugins.push(CompiledPlugin { name, dir, updated });
            }
            Err(e) => warn(format!("failed to compile plugin {name}: {e:#}")),
        }
    }
    if skip_cleanup {
        let staged = staged_plugins(root, &compiled.plugins);
        compiled.plugins.extend(staged);
    } else {
        let keep: BTreeSet<&Path> = compiled.plugins.iter().map(|p| p.dir.as_path()).collect();
        sync::reap_unlisted(root, |dir| keep.contains(dir));
    }
    compiled
}

pub(crate) fn compile_project(
    project_root: &Path,
    plugins: &[PluginSkills],
    debounce: Duration,
    warned: &mut BTreeSet<String>,
    skip_cleanup: bool,
) -> Compiled {
    if !plugins.is_empty()
        && let Err(e) = sync::ignore_in_git(&project_root.join(PROJECT_DIR))
    {
        warn(format!("skipping project plugins: {e:#}"));
        return Compiled::default();
    }
    compile(
        &project_plugins_root(project_root),
        plugins,
        debounce,
        warned,
        skip_cleanup,
    )
}

fn staged_plugins(root: &Path, compiled: &[CompiledPlugin]) -> Vec<CompiledPlugin> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| is_plugin_dir(dir) && sync::has_symposium_marker(dir))
        .filter(|dir| compiled.iter().all(|plugin| &plugin.dir != dir))
        .filter_map(|dir| {
            let name = dir.file_name()?.to_str()?.to_string();
            Some(CompiledPlugin {
                name,
                dir,
                updated: false,
            })
        })
        .collect()
}

/// Stricter than Agent Plugins names: Antigravity rejects `.`.
pub(crate) fn normalize_name(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_NAME_LEN);
    let out = out.trim_end_matches('-');
    (!out.is_empty()).then(|| out.to_string())
}

fn plugin_dir_names(plugins: &[PluginSkills]) -> Vec<Option<String>> {
    let names: Vec<Option<String>> = plugins.iter().map(|p| normalize_name(&p.name)).collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names.iter().flatten() {
        *counts.entry(name).or_default() += 1;
    }
    plugins
        .iter()
        .zip(&names)
        .map(|(plugin, name)| {
            let name = name.as_deref()?;
            if counts[name] == 1 {
                return Some(name.to_string());
            }
            let mut origins: Vec<&str> = plugin
                .skills
                .iter()
                .map(|s| s.origin_hash.as_str())
                .collect();
            origins.sort();
            Some(suffixed(name, &crate::skills::hash_origin_key(&origins)))
        })
        .collect()
}

fn suffixed(name: &str, hash: &str) -> String {
    let base = &name[..name.len().min(MAX_NAME_LEN - hash.len() - 1)];
    format!("{}-{hash}", base.trim_end_matches('-'))
}

fn skill_dirs(plugin: &PluginSkills) -> Vec<(String, &SkillWithGroupContext)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for skill in &plugin.skills {
        *counts.entry(skill.skill.name()).or_default() += 1;
    }
    plugin
        .skills
        .iter()
        .map(|skill| {
            let name = skill.skill.name();
            let dir = if counts[name] > 1 {
                format!("{name}-{}", skill.origin_hash)
            } else {
                name.to_string()
            };
            (dir, skill)
        })
        .collect()
}

fn write_plugin(
    dir: &Path,
    manifest: &serde_json::Value,
    skills: &[(String, &SkillWithGroupContext)],
    debounce: Duration,
    warned: &mut BTreeSet<String>,
) -> Result<bool> {
    let names = skills.iter().map(|(dir, _)| OsString::from(dir)).collect();
    if sync::recently_synced(dir, debounce) && skill_dir_names(dir) == names {
        return Ok(false);
    }
    let (contents, skipped) = plugin_contents(manifest, skills)?;
    let updated = sync::sync_tree(dir, &contents)?;
    if updated {
        sync::report_skipped(skipped, warned);
    }
    Ok(updated)
}

pub(crate) fn skill_dir_names(plugin_dir: &Path) -> BTreeSet<OsString> {
    let Ok(entries) = fs::read_dir(plugin_dir.join(SKILLS_DIR)) else {
        return BTreeSet::new();
    };
    entries.flatten().map(|entry| entry.file_name()).collect()
}

fn plugin_contents(
    manifest: &serde_json::Value,
    skills: &[(String, &SkillWithGroupContext)],
) -> Result<(TreeContents, Vec<String>)> {
    let manifest = serde_json::to_vec_pretty(manifest)?;
    let mut contents: TreeContents = vec![
        (PathBuf::from(MANIFEST_FILE), Some(manifest.clone())),
        (PathBuf::from(CLAUDE_MANIFEST_DIR), None),
        (
            Path::new(CLAUDE_MANIFEST_DIR).join(MANIFEST_FILE),
            Some(manifest),
        ),
        (PathBuf::from(SKILLS_DIR), None),
    ];
    let mut skipped = Vec::new();
    for (dir, skill) in skills {
        let source = skill
            .skill
            .path
            .parent()
            .with_context(|| format!("skill {dir} has no source directory"))?;
        let base = Path::new(SKILLS_DIR).join(dir);
        let (tree, skips) = sync::collect_dir_contents(source, skill.agent_plugin_root.as_deref())?;
        contents.push((base.clone(), None));
        contents.extend(tree.into_iter().map(|(rel, data)| (base.join(rel), data)));
        skipped.extend(skips);
    }
    contents.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((contents, skipped))
}

fn manifest(name: &str, version: &str) -> serde_json::Value {
    json!({
        "$schema": AGENT_PLUGINS_SCHEMA,
        "name": name,
        "version": version,
    })
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

#[cfg(test)]
mod tests;
