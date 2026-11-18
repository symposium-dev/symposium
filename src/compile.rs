use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use symposium_install::UpdateLevel;

use crate::agents::CompiledPlugin;
use crate::config::{PluginRef, PluginsConfig, Symposium};
use crate::plugins::{Plugin, PluginRegistry};
use crate::pm::{PackageId, PmRegistry};
use crate::predicate::{PredicateContextImpl, PredicateDependencies};
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
    deps: &[PackageId],
) -> Vec<PluginRef> {
    let names = crate::discovery::Names::new(plugins, deps, pms);
    let mut used = Vec::new();
    for entry in config.used.iter().filter(|entry| entry.is_global()) {
        if let Some(plugin) = crate::use_command::resolved_entry(&names, pms, entry).await {
            used.push(plugin);
        }
    }
    used
}

fn installs_globally(used: &[PluginRef], plugin: &Plugin) -> bool {
    !plugin.workspace_member && used.iter().any(|entry| entry.matches(&plugin.canonical))
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
    skills: Vec<SkillWithGroupContext>,
}

pub(crate) async fn global_plugins<D, P>(
    sym: &Symposium,
    pms: &PmRegistry,
    registry: &PluginRegistry,
    active: &[P],
    ctx: &mut PredicateContextImpl<D>,
    update: UpdateLevel,
) -> Vec<PluginSkills>
where
    D: PredicateDependencies,
    P: AsRef<Plugin>,
{
    let used = globally_used(
        &sym.config.plugins,
        pms,
        &registry.plugins,
        ctx.package_ids(),
    )
    .await;
    let names = crate::discovery::Names::new(&registry.plugins, ctx.package_ids(), pms);
    let mut spellings = names.spellings(&sym.config.plugins).await;
    let inactive = registry.plugins.iter().filter(|plugin| {
        !active
            .iter()
            .any(|active| active.as_ref().canonical == plugin.canonical)
    });
    let mut plugins = Vec::new();
    for plugin in active.iter().map(AsRef::as_ref).chain(inactive) {
        names
            .spell(&sym.config.plugins, &plugin.canonical, &mut spellings)
            .await;
        if installs_globally(&used, plugin)
            && !sym
                .config
                .plugins
                .is_disabled(&plugin.canonical, &spellings)
        {
            plugins.push(plugin.clone());
        }
    }
    ctx.set_conditions_met(true);
    plugins.retain(|plugin| plugin.applies(ctx));
    let skills = crate::skills::collect_skills(sym, &plugins, ctx, update).await;
    ctx.set_conditions_met(false);
    group_skills(plugins, skills)
}

pub(crate) fn project_plugins<P>(
    active: &[P],
    applicable: &[SkillWithGroupContext],
    global: &[PluginSkills],
) -> Vec<PluginSkills>
where
    P: AsRef<Plugin>,
{
    let global_origins: BTreeSet<&str> = global
        .iter()
        .flat_map(|plugin| &plugin.skills)
        .map(|skill| skill.origin_hash())
        .collect();
    let skills = applicable
        .iter()
        .filter(|skill| !global_origins.contains(skill.origin_hash()))
        .cloned()
        .collect();
    group_skills(
        active
            .iter()
            .map(|plugin| plugin.as_ref().clone())
            .collect(),
        skills,
    )
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
        if seen.insert((
            skill.skill().name().to_string(),
            skill.origin_hash().to_string(),
        )) {
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
    pub(crate) outcome: CompilationOutcome,
}

/// Path-free artifact changes produced while compiling native plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompilationOutcome {
    pub(crate) installed: usize,
    pub(crate) updated: usize,
    pub(crate) reaped: usize,
    pub(crate) complete: bool,
}

impl Default for CompilationOutcome {
    fn default() -> Self {
        Self {
            installed: 0,
            updated: 0,
            reaped: 0,
            complete: true,
        }
    }
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
            compiled.outcome.complete = false;
            warn(format!(
                "skipping plugin {:?}: its name has no ASCII letters or digits to name a plugin directory",
                plugin.name
            ));
            continue;
        };
        let dir = root.join(&name);
        let existed = sync::has_symposium_marker(&dir);
        match write_plugin(&dir, &name, &skill_dirs(plugin), debounce, warned) {
            Ok(updated) => {
                if updated {
                    if existed {
                        compiled.outcome.updated += 1;
                    } else {
                        compiled.outcome.installed += 1;
                    }
                }
                let origins = plugin.skills.iter().map(|s| s.origin_hash().to_string());
                compiled
                    .skill_origins
                    .insert(name.clone(), origins.collect());
                compiled.plugins.push(CompiledPlugin { name, dir, updated });
            }
            Err(e) => {
                compiled.outcome.complete = false;
                warn(format!("failed to compile plugin {name}: {e:#}"));
            }
        }
    }
    if skip_cleanup {
        let staged = staged_plugins(root, &compiled.plugins);
        compiled.outcome.complete &= staged.complete;
        compiled.plugins.extend(staged.plugins);
    } else {
        let keep: BTreeSet<&Path> = compiled.plugins.iter().map(|p| p.dir.as_path()).collect();
        let reaped = sync::reap_unlisted(root, |dir| keep.contains(dir));
        compiled.outcome.reaped = reaped.removed;
        compiled.outcome.complete &= reaped.complete;
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
        return Compiled {
            outcome: CompilationOutcome {
                complete: false,
                ..CompilationOutcome::default()
            },
            ..Compiled::default()
        };
    }
    compile(
        &project_plugins_root(project_root),
        plugins,
        debounce,
        warned,
        skip_cleanup,
    )
}

struct StagedPlugins {
    plugins: Vec<CompiledPlugin>,
    complete: bool,
}

fn staged_plugins(root: &Path, compiled: &[CompiledPlugin]) -> StagedPlugins {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StagedPlugins {
                plugins: Vec::new(),
                complete: true,
            };
        }
        Err(error) => {
            warn(format!(
                "failed to inspect staged plugins in {}: {error}",
                root.display()
            ));
            return StagedPlugins {
                plugins: Vec::new(),
                complete: false,
            };
        }
    };
    let mut plugins = Vec::new();
    let mut complete = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                complete = false;
                warn(format!(
                    "failed to inspect an entry in {}: {error}",
                    root.display()
                ));
                continue;
            }
        };
        let dir = entry.path();
        if is_plugin_dir(&dir)
            && sync::has_symposium_marker(&dir)
            && compiled.iter().all(|plugin| plugin.dir != dir)
            && let Some(name) = dir.file_name().and_then(|name| name.to_str())
        {
            plugins.push(CompiledPlugin {
                name: name.to_string(),
                dir,
                updated: false,
            });
        }
    }
    StagedPlugins { plugins, complete }
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
            let mut origins: Vec<&str> = plugin.skills.iter().map(|s| s.origin_hash()).collect();
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
        *counts.entry(skill.skill().name()).or_default() += 1;
    }
    plugin
        .skills
        .iter()
        .map(|skill| {
            let name = skill.skill().name();
            let dir = if counts[name] > 1 {
                format!("{name}-{}", skill.origin_hash())
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
) -> Result<bool> {
    let names = skills.iter().map(|(dir, _)| OsString::from(dir)).collect();
    if sync::recently_synced(dir, debounce) && skill_dir_names(dir) == names {
        return Ok(false);
    }
    let (contents, skipped) = plugin_contents(name, skills)?;
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
            .skill()
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
