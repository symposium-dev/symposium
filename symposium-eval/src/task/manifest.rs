//! Each task plugin, read once: its tree, and the parts of its `Symposium.toml` the
//! checker reads.
//!
//! The manifest must first match the whole schema `symposium-sdk` defines, since
//! Symposium rejects the whole plugin otherwise; the checker's own reader then takes
//! only the fields it needs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use symposium_sdk::manifest::RawPluginManifest;

use super::{DirProblem, PluginName, TaskError, distinct};
use crate::evals::Evals;
use crate::tree::{self, EntryKind, TreeEntry};

/// One of the task's plugins, as the checker reads it.
pub(super) struct Plugin<'a> {
    pub(super) name: &'a PluginName,
    pub(super) dir: PathBuf,
    /// Every file and directory in the plugin, which holds nothing else.
    pub(super) entries: Vec<TreeEntry>,
    /// `None` without a `Symposium.toml`.
    pub(super) manifest: Option<Manifest>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Manifest {
    #[serde(default)]
    pub(super) hooks: Vec<RawHook>,
    #[serde(default)]
    pub(super) skills: Vec<RawSkillGroup>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawHook {
    pub(super) name: String,
    pub(super) event: String,
    pub(super) agent: Option<String>,
    pub(super) format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawSkillGroup {
    /// Kept raw: only `source.path` is read, and any other form is refused.
    pub(super) source: Option<toml::Value>,
}

/// Each of `plugins` once, with its tree and manifest. `None` when a plugin cannot be
/// read, or holds what a trial cannot copy: it could declare anything, and no trial of
/// the task could start, so then only its errors are reported.
pub(super) fn read<'a>(
    evals: &Evals,
    plugins: &'a [PluginName],
    errors: &mut Vec<TaskError>,
) -> Option<Vec<Plugin<'a>>> {
    let mut read = Vec::new();
    let mut unreadable = false;
    for name in distinct(plugins) {
        let dir = evals.plugin_dir(name);
        // The task's directory checks report a plugin that is not a directory.
        if !matches!(DirProblem::of(&dir), Ok(None)) {
            unreadable = true;
            continue;
        }
        let before = errors.len();
        let entries = copyable_entries(name, &dir, errors);
        let manifest = manifest(name, &dir).unwrap_or_else(|error| {
            errors.push(error);
            None
        });
        if errors.len() > before {
            unreadable = true;
            continue;
        }
        read.push(Plugin {
            name,
            dir,
            entries,
            manifest,
        });
    }
    (!unreadable).then_some(read)
}

/// The plugin's entries, reporting each that `copy_tree` would refuse when creating a
/// trial: a link or a special file, anywhere in it.
fn copyable_entries(
    plugin: &PluginName,
    dir: &Path,
    errors: &mut Vec<TaskError>,
) -> Vec<TreeEntry> {
    let entries = match tree::walk(dir) {
        Ok(entries) => entries,
        Err(source) => {
            errors.push(TaskError::PluginTree {
                plugin: plugin.clone(),
                source,
            });
            return Vec::new();
        }
    };
    for entry in &entries {
        if matches!(entry.kind, EntryKind::Symlink | EntryKind::Other) {
            errors.push(TaskError::PluginEntry {
                plugin: plugin.clone(),
                path: entry.path.clone(),
                kind: entry.kind,
            });
        }
    }
    entries
}

fn manifest(plugin: &PluginName, dir: &Path) -> Result<Option<Manifest>, TaskError> {
    let path = dir.join("Symposium.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(TaskError::Inspect { path, source }),
    };
    let invalid = |source| TaskError::PluginManifest {
        plugin: plugin.clone(),
        source,
    };
    toml::from_str::<RawPluginManifest>(&text).map_err(invalid)?;
    toml::from_str(&text).map(Some).map_err(invalid)
}
