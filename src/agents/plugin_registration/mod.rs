pub(super) mod claude;
pub(super) mod codex;
pub(super) mod copilot;

use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use crate::agents::{Agent, CompiledPlugin, load_json_or_empty, save_json};
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::output::display_path;
use crate::report::ReportEvent;

const MARKETPLACE: &str = "symposium";
const ENABLED_SUFFIX: &str = "@symposium";

pub(crate) fn sync_marketplace_index(sym: &Symposium, plugins: &[CompiledPlugin]) {
    let read = [Agent::Codex, Agent::Copilot]
        .iter()
        .any(|agent| agent.is_configured(sym));
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
