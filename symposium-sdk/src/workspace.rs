//! Workspace dependency types and resolution.
//!
//! These types represent the crates in a workspace's dependency graph. They
//! mirror what symposium resolves internally via `cargo metadata` and may be
//! passed to plugin binaries in the future (e.g. on stdin).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use std::{fmt::Write as _, fs};

use cargo_metadata::{CargoOpt, MetadataCommand};
use serde::{Deserialize, Serialize};

/// A crate in the workspace's direct dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCrate {
    /// The crate name as published (e.g. `"serde"`, `"tokio"`).
    pub name: String,
    /// The resolved version.
    pub version: semver::Version,
    /// Local source path for path dependencies.
    /// `None` for registry crates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

/// The resolved workspace: root path + dependency list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedWorkspace {
    /// Workspace root directory.
    pub root: PathBuf,
    /// Direct dependencies of all workspace members.
    pub crates: Vec<WorkspaceCrate>,
}

/// On-disk cache format.
#[derive(Serialize, Deserialize)]
struct DiskCache {
    lock_mtime: u64,
    root: PathBuf,
    crates: Vec<WorkspaceCrate>,
}

/// In-process cache for workspace dependency resolution.
///
/// First call to `load()` checks the disk cache (keyed on `Cargo.lock` mtime);
/// on miss it runs `cargo metadata` (expensive) and writes through to disk.
/// Subsequent in-process calls return the cached Arc directly.
pub struct WorkspaceDeps {
    cwd: PathBuf,
    dirs: Option<crate::dirs::SymposiumDirs>,
    /// Lazily resolved workspace-specific cache dir. `Some(Some(..))` = resolved,
    /// `Some(None)` = resolved to "not in a workspace", `None` = not yet resolved.
    resolved_cache_dir: Option<Option<PathBuf>>,
    cached: Option<Arc<LoadedWorkspace>>,
}

impl WorkspaceDeps {
    /// Create without directory context (no disk caching, default cargo binary).
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            dirs: None,
            resolved_cache_dir: None,
            cached: None,
        }
    }

    /// Create with full directory context (enables disk caching and cargo override).
    pub fn with_dirs(cwd: impl Into<PathBuf>, dirs: &crate::dirs::SymposiumDirs) -> Self {
        Self {
            cwd: cwd.into(),
            dirs: Some(dirs.clone()),
            resolved_cache_dir: None,
            cached: None,
        }
    }

    /// Load (or return cached) workspace metadata.
    /// Returns `None` if not inside a Cargo workspace.
    pub fn load(&mut self) -> Option<&Arc<LoadedWorkspace>> {
        if self.cached.is_some() {
            return self.cached.as_ref();
        }

        // Phase 2: try disk cache first.
        if let Some(loaded) = self.try_disk_cache() {
            self.cached = Some(Arc::new(loaded));
            return self.cached.as_ref();
        }

        // Cache miss: run cargo metadata.
        let cargo_path = self.dirs.as_ref().and_then(|d| d.cargo_override.as_deref());
        let loaded = load_workspace(&self.cwd, cargo_path)?;

        // Write through to disk cache.
        self.write_disk_cache(&loaded);

        self.cached = Some(Arc::new(loaded));
        self.cached.as_ref()
    }

    /// Convenience: workspace root, or `None` if not in a workspace.
    pub fn workspace_root(&mut self) -> Option<&Path> {
        self.load().map(|w| w.root.as_path())
    }

    /// Convenience: crate list (empty slice if not in a workspace).
    pub fn crates(&mut self) -> &[WorkspaceCrate] {
        match self.load() {
            Some(w) => &w.crates,
            None => &[],
        }
    }

    /// Resolve the workspace-specific cache directory (at most once per instance).
    /// Uses `cargo locate-project --workspace` (~10ms) on first call.
    fn resolve_workspace_cache_dir(&mut self) -> Option<&Path> {
        if self.resolved_cache_dir.is_none() {
            self.resolved_cache_dir = Some(self.compute_workspace_cache_dir());
        }
        self.resolved_cache_dir.as_ref().unwrap().as_deref()
    }

    fn compute_workspace_cache_dir(&self) -> Option<PathBuf> {
        let dirs = self.dirs.as_ref()?;
        let cargo_path = dirs.cargo_override.as_deref();
        let root = locate_workspace_root(&self.cwd, cargo_path)?;
        let canonical = fs::canonicalize(&root).unwrap_or(root);
        Some(
            dirs.cache_dir
                .join("workspaces")
                .join(workspace_dir_name(&canonical)),
        )
    }

    fn try_disk_cache(&mut self) -> Option<LoadedWorkspace> {
        let ws_cache_dir = self.resolve_workspace_cache_dir()?.to_path_buf();
        let cache_file = ws_cache_dir.join("workspace-deps.json");
        let contents = fs::read_to_string(&cache_file).ok()?;
        let cached: DiskCache = serde_json::from_str(&contents).ok()?;

        // Validate: Cargo.lock mtime must match.
        let lock_path = cached.root.join("Cargo.lock");
        let current_mtime = file_mtime(&lock_path)?;
        if current_mtime != cached.lock_mtime {
            return None;
        }

        Some(LoadedWorkspace {
            root: cached.root,
            crates: cached.crates,
        })
    }

    fn write_disk_cache(&self, loaded: &LoadedWorkspace) {
        let Some(Some(ws_cache_dir)) = &self.resolved_cache_dir else {
            return;
        };
        let lock_path = loaded.root.join("Cargo.lock");
        let Some(mtime) = file_mtime(&lock_path) else {
            return;
        };

        let disk = DiskCache {
            lock_mtime: mtime,
            root: loaded.root.clone(),
            crates: loaded.crates.clone(),
        };

        let _ = fs::create_dir_all(ws_cache_dir);
        let _ = fs::write(
            ws_cache_dir.join("workspace-deps.json"),
            serde_json::to_string_pretty(&disk).unwrap_or_default(),
        );
    }
}

/// Find workspace root via `cargo locate-project --workspace`.
/// Fast (~10-50ms), no dep resolution.
fn locate_workspace_root(cwd: &Path, cargo_path: Option<&Path>) -> Option<PathBuf> {
    let cargo = cargo_path.unwrap_or(Path::new("cargo"));
    let output = std::process::Command::new(cargo)
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

/// Compute a human-readable, unique directory name for a workspace root.
///
/// Format: `<tail>-<8-hex-sha256-prefix>` where `tail` is the final path
/// component. Used to derive per-workspace cache directories.
pub fn workspace_dir_name(workspace_root: &Path) -> String {
    use sha2::{Digest, Sha256};

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

/// Get a file's mtime as seconds since the Unix epoch.
/// Returns `None` if the file doesn't exist or its metadata can't be read.
pub fn file_mtime(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(
        mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
}

/// Run `cargo metadata` and extract workspace root + direct deps.
fn load_workspace(cwd: &Path, cargo_path: Option<&Path>) -> Option<LoadedWorkspace> {
    let mut cmd = MetadataCommand::new();
    cmd.features(CargoOpt::AllFeatures).current_dir(cwd);
    if let Some(path) = cargo_path {
        cmd.cargo_path(path);
    }
    let metadata = cmd.exec().ok()?;

    let root = metadata.workspace_root.clone().into_std_path_buf();

    let resolve = metadata.resolve.as_ref()?;

    let ws_members: HashSet<_> = metadata.workspace_members.iter().collect();
    let mut direct_dep_ids: HashSet<&cargo_metadata::PackageId> = HashSet::new();

    for node in &resolve.nodes {
        if ws_members.contains(&node.id) {
            for dep in &node.deps {
                direct_dep_ids.insert(&dep.pkg);
            }
        }
    }

    let path_overrides: HashMap<String, PathBuf> = metadata
        .packages
        .iter()
        .filter(|p| p.source.is_none())
        .filter_map(|p| {
            p.manifest_path
                .parent()
                .map(|dir| (p.name.clone(), dir.into()))
        })
        .collect();

    let mut crates: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| direct_dep_ids.contains(&p.id) && !ws_members.contains(&p.id))
        .filter_map(|p| {
            semver::Version::parse(&p.version.to_string())
                .ok()
                .map(|v| WorkspaceCrate {
                    path: path_overrides.get(&p.name).cloned(),
                    name: p.name.to_string(),
                    version: v,
                })
        })
        .collect();

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);

    Some(LoadedWorkspace { root, crates })
}
