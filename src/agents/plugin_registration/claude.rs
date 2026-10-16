use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

use crate::agents::{Agent, CompiledPlugin};
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::report::ReportEvent;

pub(crate) fn sync_user_plugins(
    sym: &Symposium,
    _root: &Path,
    plugins: &[CompiledPlugin],
    _out: &Output,
) -> Result<bool> {
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    sync_skills_dir_plugins(&user_skills_dir(sym), plugins, debounce);
    Ok(true)
}

/// When set, `CLAUDE_CONFIG_DIR` replaces `~/.claude` and Claude Code ignores
/// `~/.claude/skills`.
fn user_skills_dir(sym: &Symposium) -> PathBuf {
    crate::agents::claude_config_dir(sym).join("skills")
}

fn sync_skills_dir_plugins(skills_dir: &Path, plugins: &[CompiledPlugin], debounce: Duration) {
    let mut installed = BTreeSet::new();
    for plugin in plugins {
        let Some(dest) = destination(skills_dir, &plugin.name) else {
            warn(format!(
                "skipping plugin {} for {}: {} is not managed by symposium",
                plugin.name,
                Agent::Claude.config_name(),
                display_path(&skills_dir.join(&plugin.name)),
            ));
            continue;
        };
        match crate::sync::sync_plugin_dir(&plugin.dir, &dest, debounce) {
            Ok(changed) => {
                if changed {
                    tracing::info!(
                        report = %ReportEvent::PluginInstalled {
                            plugin: plugin.name.clone(),
                            agent: Agent::Claude.config_name().to_string(),
                            dest: display_path(&dest),
                        },
                    );
                }
                installed.insert(dest);
            }
            Err(e) => warn(format!(
                "failed to install plugin {} for {}: {e:#}",
                plugin.name,
                Agent::Claude.config_name(),
            )),
        }
    }
    crate::sync::reap_unlisted(skills_dir, |dir| {
        installed.contains(dir) || !crate::compile::is_plugin_dir(dir)
    });
}

fn destination(skills_dir: &Path, name: &str) -> Option<PathBuf> {
    [
        name.to_string(),
        format!("{name}-{}", crate::skills::hash_origin_key(&name)),
    ]
    .into_iter()
    .map(|dir| skills_dir.join(dir))
    .find(|dir| !dir.exists() || crate::sync::has_symposium_marker(dir))
}

fn warn(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}

#[cfg(test)]
mod tests;
