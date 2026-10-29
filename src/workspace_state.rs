//! Per-directory persistent state: what the last sync from a directory found.
//!
//! Keyed on the directory a session starts in, not on the workspace root:
//! the root is what the package managers report, so knowing it would mean
//! asking them, which is what the recorded state lets a hook event skip.
//!
//! Stored under `~/.symposium/cache/workspaces/<name>-<hash>/state.json`.
//! The directory name uses the directory's final component for readability
//! and an 8-hex-char SHA-256 prefix of the full path for uniqueness.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::pm::{PackageId, WorkspaceInfo};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// `None` until the first successful sync.
    #[serde(default, rename = "last-sync")]
    pub last_sync: Option<LastSync>,
}

/// What a sync found: the workspace and its dependencies, with each input's
/// modification time then (`None` for a file that did not exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastSync {
    pub root: PathBuf,
    pub members: Vec<PathBuf>,
    pub deps: Vec<PackageId>,
    pub inputs: BTreeMap<PathBuf, Option<u64>>,
}

impl WorkspaceState {
    pub fn load(sym: &Symposium, dir: &Path) -> Self {
        let path = state_file_path(sym, dir);
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self, sym: &Symposium, dir: &Path) {
        let path = state_file_path(sym, dir);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, contents);
        }
    }

    /// The workspace and dependencies the last sync found, if every input is
    /// as it was then. A file that appeared or disappeared counts as changed.
    /// A workspace with no inputs is never fresh, since nothing would tell
    /// that its dependencies had changed.
    pub fn fresh(&self) -> Option<(WorkspaceInfo, Vec<PackageId>)> {
        let last = self.last_sync.as_ref()?;
        let unchanged = !last.inputs.is_empty()
            && last
                .inputs
                .iter()
                .all(|(path, mtime)| crate::pm::file_mtime(path) == *mtime);
        unchanged.then(|| {
            let info = WorkspaceInfo {
                root: last.root.clone(),
                members: last.members.clone(),
                inputs: last.inputs.keys().cloned().collect(),
            };
            (info, last.deps.clone())
        })
    }

    /// Record a sync over `info` and `deps`.
    pub fn record_sync(&mut self, info: &WorkspaceInfo, deps: &[PackageId]) {
        self.last_sync = Some(LastSync {
            root: info.root.clone(),
            members: info.members.clone(),
            deps: deps.to_vec(),
            inputs: info
                .inputs
                .iter()
                .map(|path| (path.clone(), crate::pm::file_mtime(path)))
                .collect(),
        });
    }
}

fn state_file_path(sym: &Symposium, workspace_root: &Path) -> PathBuf {
    // Canonicalize so that symlink variants (e.g. /var vs /private/var on
    // macOS) hash to the same cache directory.
    let canonical =
        fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    sym.cache_dir()
        .join("workspaces")
        .join(crate::pm::workspace_dir_name(&canonical))
        .join("state.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::{file_mtime, workspace_dir_name};

    #[test]
    fn workspace_dir_name_uses_tail_and_hash() {
        let name = workspace_dir_name(Path::new("/users/dev/symposium"));
        assert!(name.starts_with("symposium-"));
        assert_eq!(name.len(), "symposium-".len() + 8);
    }

    #[test]
    fn different_paths_same_tail_get_different_hashes() {
        let a = workspace_dir_name(Path::new("/home/alice/myproject"));
        let b = workspace_dir_name(Path::new("/home/bob/myproject"));
        assert_ne!(a, b);
        assert!(a.starts_with("myproject-"));
        assert!(b.starts_with("myproject-"));
    }

    /// A workspace whose one input is `lock` in `dir`.
    fn workspace_with_input(dir: &Path) -> WorkspaceInfo {
        WorkspaceInfo {
            root: dir.to_path_buf(),
            members: vec![dir.join("member")],
            inputs: vec![dir.join("lock")],
        }
    }

    #[test]
    fn roundtrip_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("lock"), "").unwrap();

        let mut state = WorkspaceState::default();
        state.record_sync(&workspace_with_input(&workspace), &[]);
        state.save(&sym, &workspace);

        let loaded = WorkspaceState::load(&sym, &workspace);
        assert_eq!(loaded.last_sync, state.last_sync);
        assert_eq!(
            loaded.fresh(),
            Some((workspace_with_input(&workspace), vec![]))
        );
    }

    #[test]
    fn never_synced_is_stale() {
        assert_eq!(WorkspaceState::default().fresh(), None);
    }

    /// With no inputs, nothing would show that the dependencies changed.
    #[test]
    fn a_workspace_without_inputs_is_never_fresh() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = WorkspaceState::default();
        let workspace = WorkspaceInfo {
            inputs: vec![],
            ..workspace_with_input(tmp.path())
        };
        state.record_sync(&workspace, &[]);
        assert_eq!(state.fresh(), None);
    }

    #[test]
    fn fresh_until_an_input_changes() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("lock"), "").unwrap();

        let mut state = WorkspaceState::default();
        state.record_sync(&workspace_with_input(tmp.path()), &[]);
        assert!(state.fresh().is_some());

        let last = state.last_sync.as_mut().unwrap();
        last.inputs.insert(tmp.path().join("lock"), Some(0));
        assert_eq!(state.fresh(), None);
    }

    #[test]
    fn an_input_that_appears_or_disappears_is_a_change() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("lock");

        let mut state = WorkspaceState::default();
        state.record_sync(&workspace_with_input(tmp.path()), &[]);
        assert!(state.fresh().is_some());
        fs::write(&lock, "").unwrap();
        assert_eq!(state.fresh(), None);

        state.record_sync(&workspace_with_input(tmp.path()), &[]);
        assert_eq!(
            state.last_sync.as_ref().unwrap().inputs[&lock],
            file_mtime(&lock)
        );
        fs::remove_file(&lock).unwrap();
        assert_eq!(state.fresh(), None);
    }
}
