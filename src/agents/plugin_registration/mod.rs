pub(super) mod codex;
pub(super) mod copilot;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::agents::{Agent, CompiledPlugin, load_json_or_empty, save_json};
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::output::display_path;
use crate::report::ReportEvent;

const MARKETPLACE: &str = "symposium";

pub(super) fn deliver_copies(
    sym: &Symposium,
    agent: Agent,
    plugins_dir: &Path,
    plugins: &[CompiledPlugin],
) -> Vec<String> {
    let agent = agent.config_name();
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    let mut installed = BTreeSet::new();
    let mut updated = Vec::new();
    for plugin in plugins {
        let dest = plugins_dir.join(&plugin.name);
        match crate::sync::sync_plugin_dir(&plugin.dir, &dest, debounce) {
            Ok(changed) => {
                if changed {
                    tracing::info!(
                        report = %ReportEvent::SkillInstalled {
                            skill: plugin.name.clone(),
                            agent: agent.to_string(),
                            dest: display_path(&dest),
                            plugin: true,
                        },
                    );
                    updated.push(plugin.name.clone());
                }
                installed.insert(dest);
            }
            Err(e) => tracing::info!(
                report = %ReportEvent::Warning {
                    message: format!("failed to install plugin {} for {agent}: {e:#}", plugin.name),
                },
            ),
        }
    }
    crate::sync::reap_unlisted(plugins_dir, |dir| {
        installed.contains(dir) || !crate::compile::is_plugin_dir(dir)
    });
    updated
}

pub(crate) fn sync_marketplace_index(sym: &Symposium, plugins: &[CompiledPlugin]) {
    let read = [Agent::Codex, Agent::Copilot].iter().any(|agent| {
        sym.config
            .agents
            .iter()
            .any(|entry| entry.name == agent.config_name())
    });
    let root = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    if let Err(e) = write_index(&root, if read { plugins } else { &[] }) {
        tracing::info!(
            report = %ReportEvent::Warning {
                message: format!(
                    "failed to write the plugin index in {}: {e:#}",
                    display_path(&root)
                ),
            },
        );
    }
}

fn write_index(root: &Path, plugins: &[CompiledPlugin]) -> Result<()> {
    let index = root.join(".claude-plugin").join("marketplace.json");
    if plugins.is_empty() {
        if index.exists() {
            fs::remove_file(&index)?;
            let _ = fs::remove_dir(root.join(".claude-plugin"));
        }
        return Ok(());
    }
    let value = marketplace(plugins);
    if !load_json_or_empty(&index).is_ok_and(|current| current == value) {
        save_json(&index, &value)?;
    }
    Ok(())
}

fn marketplace(plugins: &[CompiledPlugin]) -> Value {
    let mut names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    json!({
        "name": MARKETPLACE,
        "owner": { "name": MARKETPLACE },
        "plugins": names
            .into_iter()
            .map(|name| json!({ "name": name, "source": format!("./{name}") }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests;
