use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Item, Table, TableLike, value};

use crate::agents::{Agent, CompiledPlugin, codex_dir};
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::output::display_path;
use crate::report::ReportEvent;
use crate::sync;

use super::MARKETPLACE;

const ENABLED_SUFFIX: &str = "@symposium";

pub(crate) fn deliver_user_plugins(
    sym: &Symposium,
    plugins: &[CompiledPlugin],
) -> Option<Vec<String>> {
    let root = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    match deliver(&codex_dir(sym), &root, plugins, debounce) {
        Ok(updated) => Some(updated),
        Err(e) => {
            if !plugins.is_empty() {
                tracing::info!(
                    report = %ReportEvent::Warning {
                        message: format!(
                            "failed to install plugins for {}: {e:#}",
                            Agent::Codex.config_name()
                        ),
                    },
                );
            }
            None
        }
    }
}

fn deliver(
    codex_dir: &Path,
    root: &Path,
    plugins: &[CompiledPlugin],
    debounce: Duration,
) -> Result<Vec<String>> {
    let config_path = codex_dir.join("config.toml");
    let shown = display_path(&config_path);
    let original = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let mut doc: DocumentMut = original.parse().with_context(|| format!("parse {shown}"))?;
    let source = root.display().to_string();
    if doc
        .get("marketplaces")
        .and_then(|marketplaces| marketplaces.get(MARKETPLACE))
        .is_some_and(|entry| !is_ours(entry, &source))
    {
        bail!("the `{MARKETPLACE}` marketplace in {shown} is not managed by symposium");
    }

    let cache = codex_dir.join("plugins").join("cache").join(MARKETPLACE);
    let mut delivered = Vec::new();
    let mut updated = Vec::new();
    for plugin in plugins {
        let dest = cache.join(&plugin.name);
        match copy(&plugin.dir, &dest, debounce) {
            Ok(changed) => {
                if changed {
                    tracing::info!(
                        report = %ReportEvent::SkillInstalled {
                            skill: plugin.name.clone(),
                            agent: Agent::Codex.config_name().to_string(),
                            dest: display_path(&dest),
                            plugin: true,
                        },
                    );
                    updated.push(plugin.name.clone());
                }
                delivered.push(plugin);
            }
            Err(e) => tracing::info!(
                report = %ReportEvent::Warning {
                    message: format!(
                        "failed to install plugin {} for {}: {e:#}",
                        plugin.name,
                        Agent::Codex.config_name()
                    ),
                },
            ),
        }
    }
    register(&mut doc, &source, !delivered.is_empty())
        .and_then(|()| enable(&mut doc, &delivered))
        .with_context(|| format!("unexpected content in {shown}"))?;
    let rendered = doc.to_string();
    if rendered != original {
        fs::write(&config_path, rendered)?;
    }

    let keep: BTreeSet<PathBuf> = delivered.iter().map(|p| cache.join(&p.name)).collect();
    sync::reap_unlisted(&cache, |dir| keep.contains(dir));
    Ok(updated)
}

fn is_ours(entry: &Item, source: &str) -> bool {
    entry.get("source_type").and_then(Item::as_str) == Some("local")
        && entry.get("source").and_then(Item::as_str) == Some(source)
}

fn copy(source: &Path, dest: &Path, debounce: Duration) -> Result<bool> {
    if sync::recently_synced(dest, debounce) {
        return Ok(false);
    }
    let version = Path::new(crate::compile::PLUGIN_VERSION);
    let (contents, _) = sync::collect_dir_contents(source)?;
    let mut tree = vec![(version.to_path_buf(), None)];
    tree.extend(
        contents
            .into_iter()
            .map(|(rel, data)| (version.join(rel), data)),
    );
    sync::sync_tree(dest, &tree)
}

fn register(doc: &mut DocumentMut, source: &str, delivered: bool) -> Result<()> {
    if !delivered {
        if let Some(marketplaces) = doc
            .get_mut("marketplaces")
            .and_then(Item::as_table_like_mut)
        {
            marketplaces.remove(MARKETPLACE);
        }
        return Ok(());
    }
    let marketplaces = table(doc, "marketplaces")?;
    if !marketplaces.contains_key(MARKETPLACE) {
        let mut entry = Table::new();
        entry["source_type"] = value("local");
        entry["source"] = value(source);
        marketplaces.insert(MARKETPLACE, Item::Table(entry));
    }
    Ok(())
}

fn enable(doc: &mut DocumentMut, delivered: &[&CompiledPlugin]) -> Result<()> {
    let keys: BTreeSet<String> = delivered
        .iter()
        .map(|plugin| format!("{}{ENABLED_SUFFIX}", plugin.name))
        .collect();
    if keys.is_empty() && !doc.contains_key("plugins") {
        return Ok(());
    }
    let plugins = table(doc, "plugins")?;
    let stale: Vec<String> = plugins
        .iter()
        .filter(|(key, entry)| {
            key.ends_with(ENABLED_SUFFIX)
                && !keys.contains(*key)
                && entry.get("enabled").and_then(Item::as_bool) == Some(true)
        })
        .map(|(key, _)| key.to_string())
        .collect();
    for key in stale {
        plugins.remove(&key);
    }
    for key in keys {
        if !plugins.contains_key(&key) {
            let mut entry = Table::new();
            entry["enabled"] = value(true);
            plugins.insert(&key, Item::Table(entry));
        }
    }
    Ok(())
}

fn table<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut dyn TableLike> {
    doc.entry(key)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_like_mut()
        .with_context(|| format!("`{key}` is not a table"))
}

#[cfg(test)]
mod tests;
