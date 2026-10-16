use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::agents::{Agent, CompiledPlugin, load_json_or_empty, save_json};
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::report::ReportEvent;

const MARKETPLACE: &str = "symposium";
const MARKETPLACES_KEY: &str = "extraKnownMarketplaces";
const ENABLED_KEY: &str = "enabledPlugins";

pub(crate) fn sync_user_plugins(
    sym: &Symposium,
    root: &Path,
    plugins: &[CompiledPlugin],
    _out: &Output,
) -> Result<bool> {
    let path = settings_path(sym);
    let shown = display_path(&path);
    let agent = Agent::Copilot.config_name();
    let mut settings =
        load_json_or_empty(&path).with_context(|| format!("failed to read {shown}"))?;
    let before = settings.clone();
    let Some(changes) = reconcile(&mut settings, root, plugins)
        .with_context(|| format!("unexpected content in {shown}"))?
    else {
        if !plugins.is_empty() {
            warn(format!(
                "skipping plugins for {agent}: the `{MARKETPLACE}` marketplace in {shown} is not managed by symposium"
            ));
        }
        return Ok(false);
    };
    if settings != before {
        save_json(&path, &settings)?;
    }
    for plugin in changes.installed {
        tracing::info!(
            report = %ReportEvent::PluginInstalled {
                plugin,
                agent: agent.to_string(),
                dest: shown.clone(),
            },
        );
    }
    for plugin in changes.removed {
        tracing::info!(
            report = %ReportEvent::Info {
                message: format!("removed plugin {plugin} for {agent} from {shown}"),
            },
        );
    }
    Ok(true)
}

fn settings_path(sym: &Symposium) -> PathBuf {
    crate::agents::copilot_config_dir(sym).join("settings.json")
}

#[derive(Default)]
struct Changes {
    installed: Vec<String>,
    removed: Vec<String>,
}

fn reconcile(
    settings: &mut Value,
    root: &Path,
    plugins: &[CompiledPlugin],
) -> Result<Option<Changes>> {
    let Some(settings) = settings.as_object_mut() else {
        bail!("the settings are not a JSON object");
    };
    let registered_elsewhere = object(settings, MARKETPLACES_KEY)?
        .and_then(|marketplaces| marketplaces.get(MARKETPLACE))
        .is_some_and(|entry| !points_at(entry, root));
    if registered_elsewhere {
        return Ok(None);
    }

    let wanted: BTreeSet<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    let mut changes = Changes::default();
    retain(settings, ENABLED_KEY, |key, value| match plugin_name(key) {
        Some(name) if !wanted.contains(name) && *value == Value::Bool(true) => {
            changes.removed.push(name.to_string());
            false
        }
        _ => true,
    })?;
    if plugins.is_empty() {
        retain(settings, MARKETPLACES_KEY, |name, _| name != MARKETPLACE)?;
        return Ok(Some(changes));
    }

    let enabled = object_or_default(settings, ENABLED_KEY)?;
    for plugin in plugins {
        let key = format!("{}@{MARKETPLACE}", plugin.name);
        if !enabled.contains_key(&key) {
            enabled.insert(key, Value::Bool(true));
            changes.installed.push(plugin.name.clone());
        }
    }
    object_or_default(settings, MARKETPLACES_KEY)?
        .entry(MARKETPLACE)
        .or_insert_with(|| registration(root));
    Ok(Some(changes))
}

fn registration(root: &Path) -> Value {
    json!({ "source": { "source": "directory", "path": root.display().to_string() } })
}

fn points_at(entry: &Value, root: &Path) -> bool {
    let source = &entry["source"];
    source["source"] == "directory"
        && source["path"]
            .as_str()
            .is_some_and(|path| Path::new(path) == root)
}

fn plugin_name(key: &str) -> Option<&str> {
    key.strip_suffix(MARKETPLACE)?.strip_suffix('@')
}

fn object<'a>(
    settings: &'a mut Map<String, Value>,
    key: &str,
) -> Result<Option<&'a mut Map<String, Value>>> {
    match settings.get_mut(key) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => bail!("`{key}` is not an object"),
    }
}

fn object_or_default<'a>(
    settings: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    settings
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .with_context(|| format!("`{key}` is not an object"))
}

fn retain(
    settings: &mut Map<String, Value>,
    key: &str,
    keep: impl FnMut(&String, &mut Value) -> bool,
) -> Result<()> {
    let Some(map) = object(settings, key)? else {
        return Ok(());
    };
    let had_entries = !map.is_empty();
    map.retain(keep);
    if had_entries && map.is_empty() {
        settings.remove(key);
    }
    Ok(())
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

#[cfg(test)]
mod tests;
