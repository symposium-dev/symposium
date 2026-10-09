use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use crate::agents::CompiledPlugin;
use crate::config::PluginsConfig;
use crate::output::display_path;
use crate::plugins::Plugin;
use crate::report::ReportEvent;
use crate::skills::SkillWithGroupContext;
use crate::sync::{self, TreeContents};

const PLUGIN_VERSION: &str = "0.0.0";
const AGENT_PLUGINS_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const MANIFEST_FILE: &str = "plugin.json";
const CLAUDE_MANIFEST_DIR: &str = ".claude-plugin";
const MARKETPLACE_FILE: &str = "marketplace.json";
const MARKETPLACE_NAME: &str = "symposium";
const SKILLS_DIR: &str = "skills";
const MAX_NAME_LEN: usize = 64;

fn installs_globally(config: &PluginsConfig, plugin: &Plugin) -> bool {
    !plugin.workspace_member
        && [&plugin.manifest.name, &plugin.canonical.name]
            .into_iter()
            .any(|name| config.is_used_globally(name))
}

pub(crate) fn is_plugin_dir(dir: &Path) -> bool {
    dir.join(CLAUDE_MANIFEST_DIR).join(MANIFEST_FILE).is_file()
}

pub(crate) struct GlobalPlugin<'a> {
    name: &'a str,
    skills: Vec<&'a SkillWithGroupContext>,
}

pub(crate) fn global_plugins<'a>(
    config: &PluginsConfig,
    active: &'a [Plugin],
    skills: &[&'a SkillWithGroupContext],
) -> Vec<GlobalPlugin<'a>> {
    let global: Vec<bool> = active
        .iter()
        .map(|plugin| installs_globally(config, plugin))
        .collect();
    let mut grouped: BTreeMap<usize, Vec<&SkillWithGroupContext>> = BTreeMap::new();
    for skill in skills {
        if global[skill.plugin_index] {
            grouped.entry(skill.plugin_index).or_default().push(skill);
        }
    }
    grouped
        .into_iter()
        .map(|(index, skills)| GlobalPlugin {
            name: &active[index].manifest.name,
            skills,
        })
        .collect()
}

#[derive(Default)]
pub(crate) struct Compiled {
    pub plugins: Vec<CompiledPlugin>,
    pub skill_origins: BTreeSet<String>,
}

pub(crate) fn compile(root: &Path, plugins: &[GlobalPlugin], debounce: Duration) -> Compiled {
    let mut compiled = Compiled::default();
    for (plugin, name) in plugins.iter().zip(plugin_dir_names(root, plugins)) {
        let Some(name) = name else {
            warn(format!(
                "skipping plugin {:?}: its name has no letters or digits to name a plugin directory",
                plugin.name
            ));
            continue;
        };
        let skills = skill_dirs(plugin);
        if skills.is_empty() {
            continue;
        }
        let dir = root.join(&name);
        match write_plugin(&dir, &name, &skills, debounce) {
            Ok(()) => {
                compiled
                    .skill_origins
                    .extend(skills.iter().map(|(_, skill)| skill.origin_hash.clone()));
                compiled.plugins.push(CompiledPlugin {
                    name,
                    version: PLUGIN_VERSION.to_string(),
                    dir,
                });
            }
            Err(e) => warn(format!("failed to compile plugin {name}: {e:#}")),
        }
    }
    if let Err(e) = write_marketplace(root, &compiled.plugins) {
        warn(format!(
            "failed to write the plugin index in {}: {e:#}",
            display_path(root)
        ));
    }
    let keep: BTreeSet<&Path> = compiled.plugins.iter().map(|p| p.dir.as_path()).collect();
    sync::reap_unlisted(root, |dir| keep.contains(dir));
    compiled
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

fn plugin_dir_names(root: &Path, plugins: &[GlobalPlugin]) -> Vec<Option<String>> {
    let names: Vec<Option<String>> = plugins.iter().map(|p| normalize_name(p.name)).collect();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names.iter().flatten() {
        *counts.entry(name).or_default() += 1;
    }
    plugins
        .iter()
        .zip(&names)
        .map(|(plugin, name)| {
            let name = name.as_deref()?;
            let slot = root.join(name);
            let user_managed = slot.exists() && !sync::has_symposium_marker(&slot);
            if counts[name] == 1 && !user_managed {
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

fn skill_dirs<'a>(plugin: &GlobalPlugin<'a>) -> Vec<(String, &'a SkillWithGroupContext)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for skill in &plugin.skills {
        *counts.entry(skill.skill.name()).or_default() += 1;
    }
    plugin
        .skills
        .iter()
        .filter_map(|&skill| {
            let name = skill.skill.name();
            if name.trim_end_matches(['.', ' ']).is_empty() || name.contains(['/', '\\', ':']) {
                warn(format!(
                    "skipping skill {name:?} in plugin {}: not a plain directory name",
                    plugin.name
                ));
                return None;
            }
            let dir = if counts[name] > 1 {
                format!("{name}-{}", skill.origin_hash)
            } else {
                name.to_string()
            };
            Some((dir, skill))
        })
        .collect()
}

fn write_plugin(
    dir: &Path,
    name: &str,
    skills: &[(String, &SkillWithGroupContext)],
    debounce: Duration,
) -> Result<()> {
    if sync::recently_synced(dir, debounce) {
        return Ok(());
    }
    sync::sync_tree(dir, &plugin_contents(name, skills)?)?;
    Ok(())
}

fn plugin_contents(
    name: &str,
    skills: &[(String, &SkillWithGroupContext)],
) -> Result<TreeContents> {
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
    for (dir, skill) in skills {
        let source = skill
            .skill
            .path
            .parent()
            .with_context(|| format!("skill {dir} has no source directory"))?;
        let base = Path::new(SKILLS_DIR).join(dir);
        contents.push((base.clone(), None));
        for (rel, data) in sync::collect_skill_tree(source)? {
            contents.push((base.join(rel), data));
        }
    }
    contents.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(contents)
}

fn manifest(name: &str) -> serde_json::Value {
    json!({
        "$schema": AGENT_PLUGINS_SCHEMA,
        "name": name,
        "version": PLUGIN_VERSION,
    })
}

fn marketplace(plugins: &[CompiledPlugin]) -> serde_json::Value {
    let mut names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    json!({
        "name": MARKETPLACE_NAME,
        "owner": { "name": MARKETPLACE_NAME },
        "plugins": names
            .into_iter()
            .map(|name| json!({ "name": name, "source": format!("./{name}") }))
            .collect::<Vec<_>>(),
    })
}

fn write_marketplace(root: &Path, plugins: &[CompiledPlugin]) -> Result<()> {
    let dir = root.join(CLAUDE_MANIFEST_DIR);
    let path = dir.join(MARKETPLACE_FILE);
    if !plugins.is_empty() {
        crate::agents::save_json_if_changed(&path, &marketplace(plugins))?;
    } else if path.exists() {
        fs::remove_file(&path)?;
        let _ = fs::remove_dir(&dir);
    }
    Ok(())
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

#[cfg(test)]
mod tests;
