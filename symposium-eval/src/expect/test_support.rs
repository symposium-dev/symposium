//! Native delivery fixtures shared by the independent evidence readers' tests.

use std::fs;
use std::path::{Path, PathBuf};

use crate::tree::{self, EntryKind};

pub(super) struct NativeBundle {
    path: PathBuf,
}

impl NativeBundle {
    pub(super) fn new(path: &Path) -> Self {
        Self::with_version(path, "0.0.0")
    }

    pub(super) fn with_version(path: &Path, version: &str) -> Self {
        fs::create_dir_all(path.join(".claude-plugin")).unwrap();
        fs::create_dir(path.join("skills")).unwrap();
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": path.file_name().unwrap().to_str().unwrap(),
            "version": version,
        }))
        .unwrap();
        fs::write(path.join("plugin.json"), &manifest).unwrap();
        fs::write(path.join(".claude-plugin/plugin.json"), &manifest).unwrap();
        fs::write(path.join(".symposium"), "").unwrap();
        Self {
            path: path.to_owned(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn install_skill(&self, name: &str, source: &Path) -> PathBuf {
        let payload = self.path.join("skills").join(name);
        fs::create_dir(&payload).unwrap();
        for entry in tree::walk(source).unwrap() {
            if matches!(entry.path.as_str(), ".symposium" | ".gitignore") {
                continue;
            }
            let destination = payload.join(&entry.path);
            match entry.kind {
                EntryKind::Dir => fs::create_dir(destination).unwrap(),
                EntryKind::File => {
                    fs::copy(source.join(entry.path), destination).unwrap();
                }
                _ => panic!("fixture sources must contain only files and directories"),
            }
        }
        payload
    }
}
