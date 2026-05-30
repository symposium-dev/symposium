//! Per-workspace persistent state.
//!
//! Stored under `~/.symposium/cache/workspaces/<name>-<hash>/state.json`.
//! The directory name uses the workspace root's final component for
//! readability and an 8-hex-char SHA-256 prefix of the full path for
//! uniqueness.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::config::Symposium;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceState {
    #[serde(default, rename = "last-sync-lock-mtime")]
    pub last_sync_lock_mtime: Option<u64>,

    #[serde(default, rename = "workspace-root")]
    pub workspace_root: Option<PathBuf>,
}

impl WorkspaceState {
    pub fn load(sym: &Symposium, workspace_root: &Path) -> Self {
        let path = state_file_path(sym, workspace_root);
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self, sym: &Symposium, workspace_root: &Path) {
        let path = state_file_path(sym, workspace_root);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, contents);
        }
    }

    pub fn sync_is_fresh(&self, workspace_root: &Path) -> bool {
        let Some(cached_mtime) = self.last_sync_lock_mtime else {
            return false;
        };
        let Some(current_mtime) = cargo_lock_mtime(workspace_root) else {
            return false;
        };
        cached_mtime == current_mtime
    }

    pub fn record_sync(&mut self, workspace_root: &Path) {
        self.last_sync_lock_mtime = cargo_lock_mtime(workspace_root);
    }
}

/// Find the workspace root via `cargo locate-project --workspace`.
/// Fast (~10-50ms) and follows cargo's actual workspace discovery logic.
pub fn find_workspace_root(sym: &Symposium, cwd: &Path) -> Option<PathBuf> {
    let output = sym
        .cargo_command()
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let manifest = String::from_utf8(output.stdout).ok()?;
    Path::new(manifest.trim()).parent().map(|p| p.to_path_buf())
}

fn cargo_lock_mtime(workspace_root: &Path) -> Option<u64> {
    let lock_path = workspace_root.join("Cargo.lock");
    let meta = fs::metadata(&lock_path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(
        mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
}

fn workspace_dir_name(workspace_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;

    let tail = workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");

    let digest = Sha256::digest(workspace_root.as_os_str().as_encoded_bytes());
    let mut hash = String::with_capacity(8);
    for byte in &digest[..4] {
        write!(hash, "{byte:02x}").unwrap();
    }

    format!("{tail}-{hash}")
}

fn state_file_path(sym: &Symposium, workspace_root: &Path) -> PathBuf {
    sym.cache_dir()
        .join("workspaces")
        .join(workspace_dir_name(workspace_root))
        .join("state.json")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn roundtrip_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();

        let mut state = WorkspaceState::default();
        state.last_sync_lock_mtime = Some(1234567890);
        state.workspace_root = Some(workspace.clone());
        state.save(&sym, &workspace);

        let loaded = WorkspaceState::load(&sym, &workspace);
        assert_eq!(loaded.last_sync_lock_mtime, Some(1234567890));
        assert_eq!(loaded.workspace_root, Some(workspace));
    }

    #[test]
    fn sync_is_fresh_when_mtime_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("Cargo.lock"), "# lock").unwrap();

        let mtime = cargo_lock_mtime(&workspace).unwrap();
        let state = WorkspaceState {
            last_sync_lock_mtime: Some(mtime),
            workspace_root: None,
        };
        assert!(state.sync_is_fresh(&workspace));
    }

    #[test]
    fn sync_is_stale_when_mtime_differs() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("Cargo.lock"), "# lock").unwrap();

        let state = WorkspaceState {
            last_sync_lock_mtime: Some(0),
            workspace_root: None,
        };
        assert!(!state.sync_is_fresh(&workspace));
    }

    #[test]
    fn sync_is_stale_when_no_lock_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();

        let state = WorkspaceState {
            last_sync_lock_mtime: Some(1234567890),
            workspace_root: None,
        };
        assert!(!state.sync_is_fresh(&workspace));
    }

    #[test]
    fn sync_is_stale_when_no_cached_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("Cargo.lock"), "# lock").unwrap();

        let state = WorkspaceState::default();
        assert!(!state.sync_is_fresh(&workspace));
    }

    #[test]
    fn find_workspace_root_finds_workspace_from_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();

        let sym = Symposium::from_dir(tmp.path());
        let found = find_workspace_root(&sym, &src);
        assert_eq!(found, Some(root));
    }

    #[test]
    fn find_workspace_root_returns_none_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("no-rust-here");
        fs::create_dir_all(&subdir).unwrap();

        let sym = Symposium::from_dir(tmp.path());
        let found = find_workspace_root(&sym, &subdir);
        assert!(found.is_none());
    }
}
