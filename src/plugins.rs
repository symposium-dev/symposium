use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::config::plugins_dir;
use crate::hook::HookEvent;

#[derive(Debug, Deserialize, Clone)]
pub struct Plugin {
    pub name: String,
    #[serde(default)]
    pub installation: Option<Installation>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Installation {
    pub commands: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Hook {
    pub name: String,
    pub event: HookEvent,
    pub command: String,
}

/// Return all hooks (with their plugin name) that match `event`.
pub fn hooks_for_event(event: &HookEvent) -> Result<Vec<(String, Hook)>> {
    let mut out = Vec::new();
    let dir = plugins_dir();

    let plugin_results = load_plugins_from_dir(dir)?;
    for plugin_res in plugin_results {
        match plugin_res {
            Ok(plugin) => {
                let name = plugin.name.clone();
                for hook in plugin.hooks.into_iter() {
                    if hook.event == *event {
                        out.push((name.clone(), hook));
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to load plugin file"),
        }
    }

    Ok(out)
}

/// Load all plugins from a directory containing TOML plugin files.
pub fn load_plugins_from_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<Result<Plugin>>> {
    let mut plugins = Vec::new();
    let dir = dir.as_ref();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            plugins.push(Err(anyhow::anyhow!(
                "directory contains non-file entry: {}",
                path.display()
            )));
        }

        match path.extension().and_then(|s| s.to_str()) {
            Some("toml") => {
                plugins.push(from_path(&path));
            }
            other => {
                plugins.push(Err(anyhow::anyhow!(
                    "unexpected file extension for {}: {:?}",
                    path.display(),
                    other
                )));
            }
        }
    }
    Ok(plugins)
}

pub fn from_str(s: &str) -> Result<Plugin> {
    let p: Plugin = toml::from_str(s)?;
    Ok(p)
}

pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Plugin> {
    let s = fs::read_to_string(path)?;
    from_str(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
name = "example-plugin"

[installation]
summary = "Download and install helper"
commands = ["wget https://example.org/bin/tool"]

[[hooks]]
name = "test"
event = "claude:pre-tool-use"
command = "echo open"
"#;

    #[test]
    fn parse_sample() {
        let plugin = from_str(SAMPLE).expect("parse");
        assert_eq!(plugin.name, "example-plugin");
        assert_eq!(plugin.hooks.len(), 1);
    }
}
