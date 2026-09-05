//! Workspace dependency types and resolution.
//!
//! [`WorkspaceDeps`] resolves and caches the crate dependency graph for a Cargo
//! workspace. Plugin binaries obtain one via
//! [`SymposiumDirs::workspace_deps`](crate::dirs::SymposiumDirs::workspace_deps).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;
use std::{fmt::Write as _, fs};

use cargo_metadata::{CargoOpt, MetadataCommand};
use serde::{Deserialize, Serialize};

/// A crate in the workspace's direct dependency graph.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCrate {
    /// The crate name as published (e.g. `"serde"`, `"tokio"`).
    pub name: String,
    /// The resolved version.
    pub version: semver::Version,
    /// Local source path for path dependencies (unpublished, so `fetch` must
    /// resolve them locally). `None` for registry crates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The crate's extracted source directory, from `cargo metadata`'s
    /// `manifest_path`. Populated for *every* resolved dependency — registry
    /// crates included, since `cargo metadata` already extracted them — so the
    /// source can be inspected or fetched without a fresh cargo probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dir: Option<PathBuf>,
}

impl WorkspaceCrate {
    /// A crate whose source lives at `path` (a path dependency), or a registry
    /// crate when `path` is `None`. `source_dir` defaults to `path`; use
    /// [`with_source_dir`](Self::with_source_dir) for a registry crate whose
    /// extracted source is known.
    pub fn new(name: String, version: semver::Version, path: Option<PathBuf>) -> Self {
        Self {
            name,
            version,
            source_dir: path.clone(),
            path,
        }
    }

    /// Set the extracted source directory (from `cargo metadata`).
    pub fn with_source_dir(mut self, source_dir: Option<PathBuf>) -> Self {
        self.source_dir = source_dir;
        self
    }
}

/// The resolved workspace: root path + dependency list + member directories.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedWorkspace {
    /// Workspace root directory.
    pub root: PathBuf,
    /// Direct dependencies of all workspace members.
    pub crates: Vec<WorkspaceCrate>,
    /// Manifest directories of the workspace's member packages. Backs
    /// workspace-plugin discovery (a member directory may define a plugin).
    pub members: Vec<PathBuf>,
}

/// On-disk cache format. Adding a field is a compatible cache bump: an old
/// cache file fails to deserialize, reads as a miss, and is rebuilt.
#[derive(Serialize, Deserialize)]
struct DiskCache {
    lock_mtime: u64,
    root: PathBuf,
    crates: Vec<WorkspaceCrate>,
    members: Vec<PathBuf>,
}

/// Lazy, cached workspace dependency resolver.
///
/// The first `load()` checks the disk cache (keyed on `Cargo.lock` mtime); on
/// miss it runs `cargo metadata` (expensive) and writes through to disk. The
/// result is memoized in a [`OnceLock`], so resolution happens at most once per
/// instance and every method reads through a shared `&self` — which lets a
/// single resolver be shared (held as an [`Arc`] by a
/// [`CargoPm`](crate::pm::CargoPm), and read directly by core code that needs
/// the workspace root or members) rather than each caller resolving its own.
pub struct WorkspaceDeps {
    cwd: PathBuf,
    dirs: crate::dirs::SymposiumDirs,
    cached: OnceLock<Option<Arc<LoadedWorkspace>>>,
}

impl WorkspaceDeps {
    pub fn new(cwd: impl Into<PathBuf>, dirs: &crate::dirs::SymposiumDirs) -> Self {
        Self {
            cwd: cwd.into(),
            dirs: dirs.clone(),
            cached: OnceLock::new(),
        }
    }

    /// A pre-resolved resolver for tests: skips `cargo metadata` and returns
    /// exactly `crates` (rooted at `root`, no members).
    #[cfg(test)]
    pub(crate) fn fixture(root: impl Into<PathBuf>, crates: Vec<WorkspaceCrate>) -> Arc<Self> {
        let root = root.into();
        let cached = OnceLock::new();
        let _ = cached.set(Some(Arc::new(LoadedWorkspace {
            root: root.clone(),
            crates,
            members: Vec::new(),
        })));
        Arc::new(Self {
            cwd: root,
            dirs: crate::dirs::SymposiumDirs::new(PathBuf::new(), PathBuf::new(), None),
            cached,
        })
    }

    /// A resolver pre-set to "no workspace" — it never runs `cargo metadata`.
    /// Backs [`detached_managers`](crate::config::Symposium::detached_managers)
    /// for workspace-independent operations (registry listing, crates.io
    /// search).
    pub fn detached() -> Self {
        let cached = OnceLock::new();
        let _ = cached.set(None);
        Self {
            cwd: PathBuf::new(),
            dirs: crate::dirs::SymposiumDirs::new(PathBuf::new(), PathBuf::new(), None),
            cached,
        }
    }

    /// The working directory this was constructed with.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Load (or return cached) workspace metadata.
    /// Returns `None` if not inside a Cargo workspace.
    pub fn load(&self) -> Option<&Arc<LoadedWorkspace>> {
        self.cached.get_or_init(|| self.resolve()).as_ref()
    }

    /// Convenience: workspace root, or `None` if not in a workspace.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.load().map(|w| w.root.as_path())
    }

    /// Convenience: crate list (empty slice if not in a workspace).
    pub fn crates(&self) -> &[WorkspaceCrate] {
        match self.load() {
            Some(w) => &w.crates,
            None => &[],
        }
    }

    /// The one-time resolution: disk cache, then `cargo metadata` on miss.
    fn resolve(&self) -> Option<Arc<LoadedWorkspace>> {
        let cache_dir = self.workspace_cache_dir();

        if let Some(dir) = &cache_dir
            && let Some(loaded) = try_disk_cache(dir)
        {
            return Some(Arc::new(loaded));
        }

        let loaded = load_workspace(&self.cwd, self.dirs.cargo_override.as_deref())?;

        if let Some(dir) = &cache_dir {
            write_disk_cache(dir, &loaded);
        }

        Some(Arc::new(loaded))
    }

    /// The workspace-specific cache directory, via `cargo locate-project
    /// --workspace` (~10ms, no dep resolution). `None` when not in a workspace.
    fn workspace_cache_dir(&self) -> Option<PathBuf> {
        let root = locate_workspace_root(&self.cwd, self.dirs.cargo_override.as_deref())?;
        let canonical = fs::canonicalize(&root).unwrap_or(root);
        Some(
            self.dirs
                .cache_dir
                .join("workspaces")
                .join(workspace_dir_name(&canonical)),
        )
    }
}

fn try_disk_cache(ws_cache_dir: &Path) -> Option<LoadedWorkspace> {
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
        members: cached.members,
    })
}

fn write_disk_cache(ws_cache_dir: &Path, loaded: &LoadedWorkspace) {
    let lock_path = loaded.root.join("Cargo.lock");
    let Some(mtime) = file_mtime(&lock_path) else {
        return;
    };

    let disk = DiskCache {
        lock_mtime: mtime,
        root: loaded.root.clone(),
        crates: loaded.crates.clone(),
        members: loaded.members.clone(),
    };

    let _ = fs::create_dir_all(ws_cache_dir);
    let _ = fs::write(
        ws_cache_dir.join("workspace-deps.json"),
        serde_json::to_string_pretty(&disk).unwrap_or_default(),
    );
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

/// Get a file's mtime as nanoseconds since the Unix epoch.
/// Returns `None` if the file doesn't exist or its metadata can't be read.
///
/// Nanoseconds, not seconds: callers ask "did this change since I last
/// looked", and an edit followed by a check is well inside one second.
pub fn file_mtime(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(
        mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64,
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
                    source_dir: p.manifest_path.parent().map(|dir| dir.into()),
                    name: p.name.to_string(),
                    version: v,
                })
        })
        .collect();

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);

    let mut members: Vec<PathBuf> = metadata
        .packages
        .iter()
        .filter(|p| ws_members.contains(&p.id))
        .filter_map(|p| p.manifest_path.parent().map(|dir| dir.into()))
        .collect();
    members.sort();
    members.dedup();

    Some(LoadedWorkspace {
        root,
        crates,
        members,
    })
}
