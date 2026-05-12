//! Rust crate source fetching and management.
//!
//! Registry crate fetching delegates to `cargo fetch` via a dummy temporary
//! package (see [`probe`]) rather than hitting `crates.io` HTTP endpoints
//! directly. Local path dependencies short-circuit through the workspace's
//! path overrides and never touch the registry.

use std::path::PathBuf;

use anyhow::Result;

mod list;
mod probe;

pub use list::{WorkspaceCrate, workspace_crates};

/// Normalize a crate name for hyphen/underscore-insensitive comparison.
///
/// Cargo treats `foo-bar` and `foo_bar` as the same crate name (published
/// name on crates.io vs. Rust module identifier), so any name-equality check
/// against a user-supplied query should go through this normalization.
pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Result of fetching a crate's sources.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The canonical crate name (e.g. `serde_json` even if queried as `serde-json`).
    pub name: String,
    /// The exact version that was fetched.
    pub version: String,
    /// Path to the crate sources on disk.
    pub path: PathBuf,
}

/// Builder for accessing Rust crate source code.
pub struct RustCrateFetch<'a> {
    crate_name: String,
    version_spec: Option<String>,
    workspace: &'a [WorkspaceCrate],
}

impl<'a> RustCrateFetch<'a> {
    /// Create a new fetch request for the given crate name.
    pub fn new(name: &str, workspace: &'a [WorkspaceCrate]) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
            workspace,
        }
    }

    /// Specify a version constraint (e.g. `"^1.0"`, `"=1.2.3"`).
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Fetch the crate sources, returning a path to the source directory.
    ///
    /// Resolution order:
    /// 1. If the crate is a local path dependency in the workspace (and no
    ///    explicit `--version` was requested), return the path directly.
    /// 2. Otherwise, run `cargo fetch` in a temporary dummy package to
    ///    populate cargo's registry cache, then read `cargo metadata` to get
    ///    the extracted source path under `~/.cargo/registry/src/`.
    pub async fn fetch(self) -> Result<FetchResult> {
        // Check path overrides first (local path dependencies).
        if self.version_spec.is_none() {
            let normalized = normalize_crate_name(&self.crate_name);
            if let Some(wc) = self
                .workspace
                .iter()
                .find(|wc| wc.path.is_some() && normalize_crate_name(&wc.name) == normalized)
            {
                let path = wc.path.as_ref().unwrap();
                tracing::debug!(crate_name = %wc.name, path = %path.display(), "resolved from path override");
                return Ok(FetchResult {
                    name: wc.name.clone(),
                    version: wc.version.to_string(),
                    path: path.clone(),
                });
            }
        }

        let (name, version_req) = self.resolve_registry_spec();
        probe::fetch_via_cargo(&name, &version_req).await
    }

    /// Choose the `(name, version_req)` pair to put in the probe package's
    /// dependency entry when going through the registry.
    ///
    /// Precedence:
    /// 1. Explicit `--version` constraint from the caller.
    /// 2. If the crate is a direct dependency of the current workspace, pin
    ///    to that exact resolved version (`=x.y.z`).
    /// 3. Otherwise, `"*"` — cargo picks the latest compatible version.
    fn resolve_registry_spec(&self) -> (String, String) {
        if let Some(spec) = &self.version_spec {
            return (self.crate_name.clone(), spec.clone());
        }

        let normalized = normalize_crate_name(&self.crate_name);
        if let Some(wc) = self
            .workspace
            .iter()
            .find(|wc| normalize_crate_name(&wc.name) == normalized)
        {
            return (wc.name.clone(), format!("={}", wc.version));
        }

        (self.crate_name.clone(), "*".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn wc(name: &str, version: &str, path: Option<PathBuf>) -> WorkspaceCrate {
        WorkspaceCrate {
            name: name.to_string(),
            version: v(version),
            path,
        }
    }

    // -- Path override behaviour ---------------------------------------

    #[tokio::test]
    async fn fetch_uses_path_override_for_path_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my-crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my-crate");
        assert_eq!(result.version, "0.1.0");
        assert_eq!(result.path, tmp.path());
    }

    #[tokio::test]
    async fn fetch_path_override_normalizes_hyphens() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my_crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        // Query with hyphen, workspace entry uses underscore.
        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my_crate");
        assert_eq!(result.path, tmp.path());
    }

    // -- Registry spec resolution (pure, no I/O) -----------------------

    #[test]
    fn registry_spec_prefers_explicit_version() {
        let workspace = vec![wc("foo", "1.0.0", None)];
        let fetch = RustCrateFetch::new("foo", &workspace).version("^2.0");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "^2.0");
    }

    #[test]
    fn registry_spec_pins_workspace_version_exactly() {
        let workspace = vec![wc("foo", "1.2.3", None)];
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "=1.2.3");
    }

    #[test]
    fn registry_spec_normalizes_hyphens_against_workspace() {
        let workspace = vec![wc("serde_json", "1.0.0", None)];
        let fetch = RustCrateFetch::new("serde-json", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        // Canonical name from the workspace wins.
        assert_eq!(name, "serde_json");
        assert_eq!(req, "=1.0.0");
    }

    #[test]
    fn registry_spec_falls_back_to_wildcard() {
        let workspace: Vec<WorkspaceCrate> = Vec::new();
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "*");
    }

    #[test]
    fn registry_spec_is_used_when_version_specified_even_with_path_dep() {
        // Explicit version → path override is skipped → registry spec uses
        // the explicit version.
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("serde", "1.0.210", Some(tmp.path().to_path_buf()))];
        let fetch = RustCrateFetch::new("serde", &workspace).version("=99.99.99");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "serde");
        assert_eq!(req, "=99.99.99");
    }
}
