//! Handing a compiled plugin directory to an agent.
//!
//! Two mechanisms, and which applies is a property of the agent, established by
//! installing a directory and asking the running agent what it can see:
//!
//! - **Registered** — the agent is pointed at the staging root and reads it in
//!   place. Only Claude Code, which is also the only agent that can express a
//!   project-scoped plugin.
//! - **Copied** — the agent loads only from its own tree. Codex, Copilot and
//!   Gemini all require this; deleting the copy makes the skill disappear.
//!
//! Symposium writes each agent's configuration itself, as it already does for
//! hooks and MCP entries, since the auto-sync path has no terminal to prompt at.
//! Copilot is the one exception, and [`reconcile_copilot_records`] says why.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::{Agent, load_json_or_empty, save_json};
use crate::agent_plugin::{CompiledPlugin, Scope};
use crate::sync::{Marking, sync_managed_dir};

/// Marketplace names symposium owns. Used to prune entries for plugins that no
/// longer apply without disturbing a marketplace the user added themselves.
const OWNED_MARKETPLACE_PREFIX: &str = "symposium";

/// One staging root, as an agent needs to be told about it.
pub struct Registration<'a> {
    pub marketplace: &'a str,
    pub root: &'a Path,
    pub plugins: &'a [&'a CompiledPlugin],
    pub scope: Scope,
}

impl Registration<'_> {
    /// `<plugin>@<marketplace>`, the key every agent uses for enablement.
    fn qualified(&self, plugin: &CompiledPlugin) -> String {
        format!("{}@{}", plugin.manifest.name, self.marketplace)
    }

    fn qualified_names(&self) -> BTreeSet<String> {
        self.plugins.iter().map(|p| self.qualified(p)).collect()
    }
}

impl Agent {
    /// Can this agent be given a compiled plugin directory at `scope`?
    ///
    /// Only Claude Code can express a project-scoped plugin; the other three
    /// store plugins per user with no way to bound them to one project, so a
    /// project-scoped plugin reaches them through the per-skill path instead.
    /// OpenCode extends through TypeScript modules and Goose through MCP
    /// servers, so neither has a directory-shaped unit at all; Kiro's is not
    /// verified yet.
    pub fn accepts_plugin_scope(&self, scope: Scope) -> bool {
        match self {
            Agent::Claude => true,
            Agent::Codex | Agent::Copilot | Agent::Gemini => scope == Scope::Global,
            Agent::Goose | Agent::Kiro | Agent::OpenCode => false,
        }
    }

    /// Install the plugins in one staging root, returning the directories
    /// written inside the agent's own tree (empty when the agent reads the
    /// staging root in place).
    pub fn install_plugins(
        &self,
        reg: &Registration,
        home: &Path,
        project_root: &Path,
        debounce: Duration,
    ) -> Result<Vec<PathBuf>> {
        match self {
            Agent::Claude => install_claude(reg, home, project_root).map(|()| Vec::new()),
            Agent::Codex => install_codex(reg, home, debounce),
            Agent::Copilot => install_copilot(reg, home, debounce),
            Agent::Gemini => install_gemini(reg, home, debounce),
            Agent::Goose | Agent::Kiro | Agent::OpenCode => Ok(Vec::new()),
        }
    }

    /// Directories to scan for copies symposium no longer owns. Empty for an
    /// agent that reads the staging root in place.
    pub fn plugin_reap_roots(&self, home: &Path) -> Vec<PathBuf> {
        match self {
            Agent::Codex => vec![home.join(".codex").join("plugins").join("cache")],
            Agent::Copilot => vec![home.join(".copilot").join("installed-plugins")],
            Agent::Gemini => vec![home.join(".gemini").join("extensions")],
            Agent::Claude | Agent::Goose | Agent::Kiro | Agent::OpenCode => Vec::new(),
        }
    }
}

fn directory_source(root: &Path) -> Value {
    json!({ "source": "directory", "path": root.display().to_string() })
}

/// Is this an entry symposium wrote, i.e. does its marketplace belong to us?
fn ours(qualified: &str) -> bool {
    qualified
        .split_once('@')
        .is_some_and(|(_, market)| market.starts_with(OWNED_MARKETPLACE_PREFIX))
}

/// Set the entries in `keep` and drop any other entry of ours, leaving entries
/// from marketplaces we do not own untouched.
fn reconcile_enabled(settings: &mut Value, keep: &BTreeSet<String>) {
    let map = settings
        .as_object_mut()
        .expect("settings is an object")
        .entry("enabledPlugins")
        .or_insert_with(|| json!({}));
    let Some(map) = map.as_object_mut() else {
        return;
    };
    map.retain(|key, _| !ours(key) || keep.contains(key));
    for key in keep {
        map.insert(key.clone(), Value::Bool(true));
    }
}

/// Register `root` as a marketplace, or drop the registration when it holds no
/// plugins. Shared by Claude Code and Copilot, which use the same key.
fn reconcile_marketplace(settings: &mut Value, reg: &Registration) {
    let map = settings
        .as_object_mut()
        .expect("settings is an object")
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| json!({}));
    let Some(map) = map.as_object_mut() else {
        return;
    };
    if reg.plugins.is_empty() {
        map.remove(reg.marketplace);
    } else {
        map.insert(
            reg.marketplace.to_string(),
            json!({ "source": directory_source(reg.root) }),
        );
    }
}

/// Claude Code resolves a directory marketplace from its registered location, so
/// nothing is copied. Both the settings entry and `known_marketplaces.json` are
/// required: with the latter missing the plugin does not load, and Claude only
/// regenerates it from settings in time for the *next* session.
fn install_claude(reg: &Registration, home: &Path, project_root: &Path) -> Result<()> {
    let user_settings = home.join(".claude").join("settings.json");
    let mut settings = load_json_or_empty(&user_settings)?;
    reconcile_marketplace(&mut settings, reg);

    let known_path = home
        .join(".claude")
        .join("plugins")
        .join("known_marketplaces.json");
    let mut known = load_json_or_empty(&known_path)?;
    if let Some(map) = known.as_object_mut() {
        if reg.plugins.is_empty() {
            map.remove(reg.marketplace);
        } else {
            let last_updated = map
                .get(reg.marketplace)
                .and_then(|entry| entry.get("lastUpdated").cloned())
                .unwrap_or_else(|| json!(now_rfc3339()));
            map.insert(
                reg.marketplace.to_string(),
                json!({
                    "source": directory_source(reg.root),
                    "installLocation": reg.root.display().to_string(),
                    "lastUpdated": last_updated,
                }),
            );
        }
    }
    save_json(&known_path, &known)?;

    let keep = reg.qualified_names();
    match reg.scope {
        Scope::Global => {
            reconcile_enabled(&mut settings, &keep);
            save_json(&user_settings, &settings)
        }
        Scope::Project => {
            save_json(&user_settings, &settings)?;
            let project_settings = project_root.join(".claude").join("settings.json");
            let mut project = load_json_or_empty(&project_settings)?;
            reconcile_enabled(&mut project, &keep);
            save_json(&project_settings, &project)
        }
    }
}

/// Codex keys its plugin cache on the version, which is why the compiled
/// manifest always carries one: the copy lands where we said rather than at a
/// default Codex picks for a version-less plugin.
fn install_codex(reg: &Registration, home: &Path, debounce: Duration) -> Result<Vec<PathBuf>> {
    let config_path = home.join(".codex").join("config.toml");
    let mut doc = load_toml_or_empty(&config_path)?;

    reconcile_codex_marketplace(&mut doc, reg);
    reconcile_codex_plugins(&mut doc, reg);
    save_toml(&config_path, &doc)?;

    let cache = home
        .join(".codex")
        .join("plugins")
        .join("cache")
        .join(reg.marketplace);
    copy_each(reg, home, debounce, |plugin| {
        cache
            .join(&plugin.manifest.name)
            .join(&plugin.manifest.version)
    })
}

fn reconcile_codex_marketplace(doc: &mut toml_edit::DocumentMut, reg: &Registration) {
    let marketplaces = doc["marketplaces"].or_insert(toml_edit::table());
    let Some(table) = marketplaces.as_table_like_mut() else {
        return;
    };
    if reg.plugins.is_empty() {
        table.remove(reg.marketplace);
        return;
    }
    let last_updated = table
        .get(reg.marketplace)
        .and_then(|entry| entry.as_table_like())
        .and_then(|entry| entry.get("last_updated"))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(now_rfc3339);

    let mut entry = toml_edit::Table::new();
    entry.insert("source_type", toml_edit::value("local"));
    entry.insert("source", toml_edit::value(reg.root.display().to_string()));
    entry.insert("last_updated", toml_edit::value(last_updated));
    table.insert(reg.marketplace, toml_edit::Item::Table(entry));
}

fn reconcile_codex_plugins(doc: &mut toml_edit::DocumentMut, reg: &Registration) {
    let plugins = doc["plugins"].or_insert(toml_edit::table());
    let Some(table) = plugins.as_table_like_mut() else {
        return;
    };
    let keep = reg.qualified_names();
    let stale: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| ours(key) && !keep.contains(key))
        .collect();
    for key in stale {
        table.remove(&key);
    }
    for key in &keep {
        let mut entry = toml_edit::Table::new();
        entry.insert("enabled", toml_edit::value(true));
        table.insert(key, toml_edit::Item::Table(entry));
    }
}

fn install_copilot(reg: &Registration, home: &Path, debounce: Duration) -> Result<Vec<PathBuf>> {
    let settings_path = home.join(".copilot").join("settings.json");
    let mut settings = load_json_or_empty(&settings_path)?;
    reconcile_marketplace(&mut settings, reg);
    reconcile_enabled(&mut settings, &reg.qualified_names());
    save_json(&settings_path, &settings)?;

    let installed = home
        .join(".copilot")
        .join("installed-plugins")
        .join(reg.marketplace);
    let written = copy_each(reg, home, debounce, |plugin| {
        installed.join(&plugin.manifest.name)
    })?;

    reconcile_copilot_records(reg, home);
    Ok(written)
}

/// Copilot only treats a plugin as installed once it appears in
/// `~/.copilot/config.json`, and that record carries a `source_sha` it computes
/// itself. Guessing that hash would couple us to an internal we cannot verify,
/// so this is the one agent where symposium drives the CLI instead of writing
/// the file: `copilot plugin install` needs no terminal, and by this point the
/// marketplace registration it resolves against is already in place.
///
/// Verified the hard way — with the settings entries and the copy present but no
/// such record, Copilot reports the skill as absent.
fn reconcile_copilot_records(reg: &Registration, home: &Path) {
    let recorded = copilot_recorded_plugins(home);
    let keep = reg.qualified_names();

    for stale in recorded.iter().filter(|r| ours(r) && !keep.contains(*r)) {
        let name = stale.split_once('@').map_or(stale.as_str(), |(n, _)| n);
        run_copilot(home, &["plugin", "uninstall", name]);
    }
    for plugin in reg.plugins {
        let qualified = reg.qualified(plugin);
        if !recorded.contains(&qualified) {
            run_copilot(home, &["plugin", "install", &qualified]);
        }
    }
}

/// The `<plugin>@<marketplace>` keys Copilot currently records as installed.
///
/// Its `config.json` is machine-managed and carries `//` comment lines, so it is
/// read leniently: an unreadable file just means nothing is recorded yet.
fn copilot_recorded_plugins(home: &Path) -> BTreeSet<String> {
    let path = home.join(".copilot").join("config.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    let body: String = raw
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return BTreeSet::new();
    };
    value
        .get("installedPlugins")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?;
                    let market = entry.get("marketplace")?.as_str()?;
                    Some(format!("{name}@{market}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Best-effort: a missing or failing `copilot` leaves the config we wrote in
/// place, and the next sync tries again.
///
/// Not run under `cargo test`. Spawning the developer's own agent CLI from a
/// unit test would make the suite depend on which binaries happen to be
/// installed, and on their being fast and non-interactive. Everything around
/// this call is tested; that Copilot then loads the plugin was established by
/// asking the running agent.
#[cfg(test)]
fn run_copilot(_home: &Path, _args: &[&str]) {}

#[cfg(not(test))]
fn run_copilot(home: &Path, args: &[&str]) {
    let result = std::process::Command::new("copilot")
        .args(args)
        .env("HOME", home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::debug!(?args, ?status, "copilot plugin command failed"),
        Err(e) => tracing::debug!(?args, error = %e, "could not run copilot"),
    }
}

/// Gemini discovers extensions by their presence in its directory, so the copy
/// is the whole installation. No configuration is written.
fn install_gemini(reg: &Registration, home: &Path, debounce: Duration) -> Result<Vec<PathBuf>> {
    let extensions = home.join(".gemini").join("extensions");
    copy_each(reg, home, debounce, |plugin| {
        extensions.join(&plugin.dir_name)
    })
}

fn copy_each(
    reg: &Registration,
    home: &Path,
    debounce: Duration,
    dest_of: impl Fn(&CompiledPlugin) -> PathBuf,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for plugin in reg.plugins {
        let source = reg.root.join(&plugin.dir_name);
        let dest = dest_of(plugin);
        sync_managed_dir(&source, &dest, home, debounce, Marking::MarkerOnly)
            .with_context(|| format!("install {} into {}", plugin.dir_name, dest.display()))?;
        written.push(dest);
    }
    Ok(written)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn load_toml_or_empty(path: &Path) -> Result<toml_edit::DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml_edit::DocumentMut::new()),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn save_toml(path: &Path, doc: &toml_edit::DocumentMut) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(path, doc.to_string()).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests;
