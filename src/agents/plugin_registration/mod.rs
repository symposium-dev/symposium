pub(super) mod antigravity;
pub(super) mod claude;
pub(super) mod codex;
pub(super) mod copilot;
pub(super) mod goose;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use crate::agents::{Agent, CompiledPlugin};
use crate::config::Symposium;
use crate::output::display_path;
use crate::report::ReportEvent;

fn sync_plugin_copies(
    sym: &Symposium,
    agent: Agent,
    plugins_dir: &Path,
    plugins: &[CompiledPlugin],
) {
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    let mut installed = BTreeSet::new();
    for plugin in plugins {
        let dest = plugins_dir.join(&plugin.name);
        match crate::sync::sync_plugin_dir(&plugin.dir, &dest, debounce) {
            Ok(changed) => {
                if changed {
                    tracing::info!(
                        report = %ReportEvent::PluginInstalled {
                            plugin: plugin.name.clone(),
                            agent: agent.config_name().to_string(),
                            dest: display_path(&dest),
                        },
                    );
                }
                installed.insert(dest);
            }
            Err(e) => tracing::info!(
                report = %ReportEvent::Warning {
                    message: format!(
                        "skipping plugin {} for {}: {e:#}",
                        plugin.name,
                        agent.config_name()
                    ),
                },
            ),
        }
    }
    crate::sync::reap_unlisted(plugins_dir, |dir| installed.contains(dir));
}

#[cfg(test)]
mod tests;
