use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Item, Table, TableLike, value};

use crate::agents::{Agent, CompiledPlugin};
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::report::ReportEvent;
use crate::sync;

const MARKETPLACE: &str = "symposium";
const MARKETPLACE_MANIFEST: &str = ".claude-plugin/marketplace.json";

pub(crate) fn sync_user_plugins(
    sym: &Symposium,
    root: &Path,
    plugins: &[CompiledPlugin],
    _out: &Output,
) -> Result<bool> {
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    sync_codex_plugins(&codex_dir(sym), root, plugins, debounce)
}

fn codex_dir(sym: &Symposium) -> PathBuf {
    sym.env_dir("CODEX_HOME")
        .unwrap_or_else(|| sym.home_dir().join(".codex"))
}

fn sync_codex_plugins(
    codex_dir: &Path,
    root: &Path,
    plugins: &[CompiledPlugin],
    debounce: Duration,
) -> Result<bool> {
    let config_path = codex_dir.join("config.toml");
    let original = read_config(&config_path)?;
    let mut doc: DocumentMut = original
        .parse()
        .with_context(|| format!("parse {}", display_path(&config_path)))?;
    let source = root
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", root.display()))?;
    if marketplace_entry(&doc).is_some_and(|entry| !is_ours(entry, source)) {
        if plugins.is_empty() {
            return Ok(false);
        }
        bail!(
            "{} registers another marketplace named `{MARKETPLACE}`",
            display_path(&config_path)
        );
    }

    let cache = codex_dir.join("plugins").join("cache").join(MARKETPLACE);
    let delivered: Vec<&CompiledPlugin> = plugins
        .iter()
        .filter(|plugin| install(&cache, plugin, debounce))
        .collect();

    let register = !delivered.is_empty() && root.join(MARKETPLACE_MANIFEST).is_file();
    set_marketplace(&mut doc, register.then_some(source))?;
    set_enabled_plugins(&mut doc, &delivered)?;
    let updated = doc.to_string();
    if updated != original {
        write_config(&config_path, &updated)?;
    }

    let keep: BTreeSet<PathBuf> = delivered.iter().map(|p| cache.join(&p.name)).collect();
    sync::reap_unlisted(&cache, |dir| keep.contains(dir));
    if delivered.is_empty() {
        let _ = fs::remove_dir(&cache);
    }
    Ok(true)
}

fn read_config(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("read {}", display_path(path))),
    }
}

/// Renaming onto a symlink would replace a user's linked `config.toml` with a
/// plain file, so the link target is written instead, as Codex itself does.
fn write_config(path: &Path, contents: &str) -> Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let temp = target.with_extension(format!("symposium-tmp-{}", std::process::id()));
    fs::write(&temp, contents).with_context(|| format!("write {}", temp.display()))?;
    if let Err(e) = fs::rename(&temp, &target) {
        let _ = fs::remove_file(&temp);
        return Err(e).with_context(|| format!("write {}", display_path(path)));
    }
    Ok(())
}

fn marketplace_entry(doc: &DocumentMut) -> Option<&Item> {
    doc.get("marketplaces")?.get(MARKETPLACE)
}

fn is_ours(entry: &Item, source: &str) -> bool {
    entry.get("source_type").and_then(Item::as_str) == Some("local")
        && entry.get("source").and_then(Item::as_str) == Some(source)
}

fn install(cache: &Path, plugin: &CompiledPlugin, debounce: Duration) -> bool {
    let dest = cache.join(&plugin.name);
    match copy_into_cache(&dest, plugin, debounce) {
        Ok(changed) => {
            if changed {
                tracing::info!(
                    report = %ReportEvent::PluginInstalled {
                        plugin: plugin.name.clone(),
                        agent: Agent::Codex.config_name().to_string(),
                        dest: display_path(&dest.join(&plugin.version)),
                    },
                );
            }
            true
        }
        Err(e) => {
            tracing::info!(
                report = %ReportEvent::Warning {
                    message: format!(
                        "failed to install plugin {} for {}: {e:#}",
                        plugin.name,
                        Agent::Codex.config_name(),
                    ),
                },
            );
            false
        }
    }
}

/// The whole `<plugin>/` directory is swapped, never a version directory inside
/// it: Codex loads whichever directory there sorts highest, a temporary one
/// included.
fn copy_into_cache(dest: &Path, plugin: &CompiledPlugin, debounce: Duration) -> Result<bool> {
    if sync::recently_synced(dest, debounce) {
        return Ok(false);
    }
    // A copy Codex installed itself from the staging root carries no marker.
    if dest.exists() && !sync::has_symposium_marker(dest) {
        fs::remove_dir_all(dest).with_context(|| format!("remove {}", display_path(dest)))?;
    }
    let version = Path::new(&plugin.version);
    let mut tree = vec![(version.to_path_buf(), None)];
    tree.extend(
        sync::collect_skill_tree(&plugin.dir)?
            .into_iter()
            .map(|(rel, data)| (version.join(rel), data)),
    );
    sync::sync_tree(dest, &tree)
}

fn set_marketplace(doc: &mut DocumentMut, source: Option<&str>) -> Result<()> {
    match source {
        Some(source) => {
            let marketplaces = table_mut(doc, "marketplaces")?;
            if !marketplaces.contains_key(MARKETPLACE) {
                let mut entry = Table::new();
                entry["source_type"] = value("local");
                entry["source"] = value(source);
                marketplaces.insert(MARKETPLACE, Item::Table(entry));
            }
        }
        None => {
            if let Some(marketplaces) = doc
                .get_mut("marketplaces")
                .and_then(Item::as_table_like_mut)
            {
                marketplaces.remove(MARKETPLACE);
            }
        }
    }
    Ok(())
}

fn set_enabled_plugins(doc: &mut DocumentMut, delivered: &[&CompiledPlugin]) -> Result<()> {
    let keys: BTreeSet<String> = delivered
        .iter()
        .map(|plugin| format!("{}@{MARKETPLACE}", plugin.name))
        .collect();
    if keys.is_empty() && !doc.contains_key("plugins") {
        return Ok(());
    }
    let plugins = table_mut(doc, "plugins")?;
    let suffix = format!("@{MARKETPLACE}");
    let stale: Vec<String> = plugins
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| key.ends_with(&suffix) && !keys.contains(key))
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

fn table_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut dyn TableLike> {
    doc.entry(key)
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_like_mut()
        .with_context(|| format!("`{key}` in Codex's config.toml is not a table"))
}

#[cfg(test)]
mod tests;
