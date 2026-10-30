use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::agents::{Agent, CompiledPlugin, copilot_config_dir, load_json_or_empty, save_json};
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::output::display_path;
use crate::report::ReportEvent;

use super::MARKETPLACE;

const ENABLED_SUFFIX: &str = "@symposium";
const MARKETPLACES_KEY: &str = "extraKnownMarketplaces";
const ENABLED_KEY: &str = "enabledPlugins";

pub(crate) fn deliver_user_plugins(
    sym: &Symposium,
    plugins: &[CompiledPlugin],
) -> Option<Vec<String>> {
    let root = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    let settings = copilot_config_dir(sym).join("settings.json");
    match deliver(&root, &settings, plugins) {
        Ok(installed) => Some(installed),
        Err(e) => {
            if !plugins.is_empty() {
                tracing::info!(
                    report = %ReportEvent::Warning {
                        message: format!(
                            "failed to install plugins for {}: {e:#}",
                            Agent::Copilot.config_name()
                        ),
                    },
                );
            }
            None
        }
    }
}

fn deliver(root: &Path, settings_path: &Path, plugins: &[CompiledPlugin]) -> Result<Vec<String>> {
    let shown = display_path(settings_path);
    let mut settings =
        load_json_or_empty(settings_path).with_context(|| format!("failed to read {shown}"))?;
    let before = settings.clone();
    let (installed, removed) = register(&mut settings, root, plugins)
        .with_context(|| format!("unexpected content in {shown}"))?;
    if settings != before {
        save_json(settings_path, &settings)?;
    }
    for skill in &installed {
        tracing::info!(
            report = %ReportEvent::SkillInstalled {
                skill: skill.clone(),
                agent: Agent::Copilot.config_name().to_string(),
                dest: shown.clone(),
                plugin: true,
            },
        );
    }
    for key in removed {
        tracing::info!(
            report = %ReportEvent::SkillRemoved {
                path: format!("{key} from {shown}"),
            },
        );
    }
    Ok(installed)
}

fn register(
    settings: &mut Value,
    root: &Path,
    plugins: &[CompiledPlugin],
) -> Result<(Vec<String>, Vec<String>)> {
    let settings = settings
        .as_object_mut()
        .context("the settings are not a JSON object")?;
    let ours = json!({ "source": "directory", "path": root.display().to_string() });
    if settings
        .get(MARKETPLACES_KEY)
        .and_then(|marketplaces| marketplaces.get(MARKETPLACE))
        .is_some_and(|entry| entry["source"] != ours)
    {
        bail!("its `{MARKETPLACE}` marketplace is not managed by symposium");
    }

    let keys: Vec<String> = plugins
        .iter()
        .map(|p| format!("{}{ENABLED_SUFFIX}", p.name))
        .collect();
    let mut removed = Vec::new();
    if let Some(Value::Object(enabled)) = settings.get_mut(ENABLED_KEY) {
        enabled.retain(|key, value| {
            let stale =
                !keys.contains(key) && key.ends_with(ENABLED_SUFFIX) && *value == Value::Bool(true);
            if stale {
                removed.push(key.clone());
            }
            !stale
        });
    }
    if plugins.is_empty() {
        if let Some(Value::Object(marketplaces)) = settings.get_mut(MARKETPLACES_KEY) {
            marketplaces.remove(MARKETPLACE);
        }
        return Ok((Vec::new(), removed));
    }

    let enabled = object(settings, ENABLED_KEY)?;
    let mut installed = Vec::new();
    for (plugin, key) in plugins.iter().zip(keys) {
        if !enabled.contains_key(&key) {
            enabled.insert(key, Value::Bool(true));
            installed.push(plugin.name.clone());
        }
    }
    object(settings, MARKETPLACES_KEY)?
        .entry(MARKETPLACE)
        .or_insert_with(|| json!({ "source": ours }));
    Ok((installed, removed))
}

fn object<'a>(
    settings: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    settings
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("`{key}` is not an object"))
}

#[cfg(test)]
mod tests;
