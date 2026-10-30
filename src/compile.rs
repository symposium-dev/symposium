use std::collections::{BTreeMap, BTreeSet};
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

pub(crate) const PLUGIN_VERSION: &str = "0.0.0";
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

fn installs_globally(pms: &PmRegistry, used: &[PackageId], plugin: &Plugin) -> bool {
    !plugin.workspace_member && used.contains(&pms.identity(&plugin.canonical))
}

pub(crate) fn is_plugin_dir(dir: &Path) -> bool {
    dir.join(CLAUDE_MANIFEST_DIR).join(MANIFEST_FILE).is_file()
}

pub(crate) fn project_plugins_root(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_DIR).join(PROJECT_PLUGINS_SUBDIR)
}

pub(crate) struct PluginSkills {
    name: String,
    skills: Vec<SkillWithGroupContext>,
}

pub(crate) async fn global_plugins(
    sym: &Symposium,
    pms: &PmRegistry,
    registry: &PluginRegistry,
    active: &[Plugin],
    ctx: &mut PredicateContext<'_>,
    update: UpdateLevel,
) -> Vec<PluginSkills> {
    let used = globally_used(&sym.config.plugins, pms, &registry.plugins).await;
    let inactive = registry
        .plugins
        .iter()
        .filter(|plugin| !active.iter().any(|a| a.canonical == plugin.canonical));
    let mut plugins: Vec<Plugin> = active
        .iter()
        .chain(inactive)
        .filter(|plugin| installs_globally(pms, &used, plugin))
        .cloned()
        .collect();
    ctx.set_dependencies_satisfied(true);
    plugins.retain(|plugin| plugin.applies(ctx));
    let skills = crate::skills::collect_skills(sym, &plugins, ctx, update).await;
    ctx.set_dependencies_satisfied(false);
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
    pub skill_origins: BTreeSet<String>,
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
        match write_plugin(&dir, &name, &skill_dirs(plugin), debounce, warned) {
            Ok(()) => {
                compiled
                    .skill_origins
                    .extend(plugin.skills.iter().map(|skill| skill.origin_hash.clone()));
                compiled.plugins.push(CompiledPlugin { name, dir });
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
            Some(CompiledPlugin { name, dir })
        })
        .collect()
}

/// Stricter than Agent Plugins names: Antigravity rejects `.`.
fn normalize_name(name: &str) -> Option<String> {
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
    name: &str,
    skills: &[(String, &SkillWithGroupContext)],
    debounce: Duration,
    warned: &mut BTreeSet<String>,
) -> Result<()> {
    if sync::recently_synced(dir, debounce) {
        return Ok(());
    }
    let (contents, skipped) = plugin_contents(name, skills)?;
    if sync::sync_tree(dir, &contents)? {
        sync::report_skipped(skipped, warned);
    }
    Ok(())
}

fn plugin_contents(
    name: &str,
    skills: &[(String, &SkillWithGroupContext)],
) -> Result<(TreeContents, Vec<String>)> {
    let manifest = serde_json::to_vec_pretty(&manifest(name))?;
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
        let (tree, skips) = sync::collect_dir_contents(source)?;
        contents.push((base.clone(), None));
        contents.extend(tree.into_iter().map(|(rel, data)| (base.join(rel), data)));
        skipped.extend(skips);
    }
    contents.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((contents, skipped))
}

fn manifest(name: &str) -> serde_json::Value {
    json!({
        "$schema": AGENT_PLUGINS_SCHEMA,
        "name": name,
        "version": PLUGIN_VERSION,
    })
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

#[cfg(test)]
mod tests;
