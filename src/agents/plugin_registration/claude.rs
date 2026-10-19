use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::agents::{Agent, CompiledPlugin};
use crate::config::Symposium;
use crate::output::display_path;
use crate::report::ReportEvent;

pub(crate) fn deliver_user_plugins(sym: &Symposium, plugins: &[CompiledPlugin]) -> Vec<String> {
    let skills_dir = user_skills_dir(sym);
    let agent = Agent::Claude.config_name();
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    let mut installed = BTreeSet::new();
    let mut updated = Vec::new();
    for plugin in plugins {
        let dest = skills_dir.join(&plugin.name);
        match crate::sync::sync_plugin_dir(&plugin.dir, &dest, debounce) {
            Ok(changed) => {
                if changed {
                    tracing::info!(
                        report = %ReportEvent::PluginInstalled {
                            plugin: plugin.name.clone(),
                            agent: agent.to_string(),
                            dest: display_path(&dest),
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
    crate::sync::reap_unlisted(&skills_dir, |dir| {
        installed.contains(dir) || !crate::compile::is_plugin_dir(dir)
    });
    updated
}

/// When set, `CLAUDE_CONFIG_DIR` replaces `~/.claude` and Claude Code ignores
/// `~/.claude/skills`.
fn user_skills_dir(sym: &Symposium) -> PathBuf {
    crate::agents::claude_config_dir(sym).join("skills")
}

#[cfg(test)]
mod tests;
