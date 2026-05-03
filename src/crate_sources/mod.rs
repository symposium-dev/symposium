//! Rust crate source fetching and management

use std::path::{Path, PathBuf};

use anyhow::Result;

mod cache;
mod extraction;
mod list;
mod version;

pub use list::{WorkspaceCrate, workspace_crates};

/// Result of fetching a crate's sources
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The canonical crate name (e.g., `serde_json` even if queried as `serde-json`)
    pub name: String,
    /// The exact version that was fetched
    pub version: String,
    /// Path to the crate sources on disk
    pub path: PathBuf,
}

/// Builder for accessing Rust crate source code
pub struct RustCrateFetch<'a> {
    crate_name: String,
    version_spec: Option<String>,
    workspace: &'a [WorkspaceCrate],
    cache_dir: &'a Path,
}

impl<'a> RustCrateFetch<'a> {
    /// Create a new fetch request for the given crate name
    pub fn new(name: &str, workspace: &'a [WorkspaceCrate], cache_dir: &'a Path) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
            workspace,
            cache_dir,
        }
    }

    /// Specify a version constraint (e.g., "^1.0", "=1.2.3")
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Fetch the crate sources, returning the path to extracted sources
    pub async fn fetch(self) -> Result<FetchResult> {
        // Check path overrides first (local path dependencies).
        if self.version_spec.is_none() {
            let normalized = self.crate_name.replace('-', "_");
            if let Some(wc) = self
                .workspace
                .iter()
                .find(|wc| wc.path.is_some() && wc.name.replace('-', "_") == normalized)
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

        let semver_pairs: Vec<(String, semver::Version)> = self
            .workspace
            .iter()
            .map(|wc| (wc.name.clone(), wc.version.clone()))
            .collect();
        let resolver = version::VersionResolver::new(&semver_pairs);
        let (canonical_name, version) = resolver
            .resolve(&self.crate_name, self.version_spec.as_deref())
            .await?;

        let cache_manager = cache::CacheManager::new(self.cache_dir)?;
        let extractor = extraction::CrateExtractor::new();

        let path = cache_manager
            .get_or_extract_crate(&canonical_name, &version, &extractor)
            .await?;

        Ok(FetchResult {
            name: canonical_name,
            version,
            path,
        })
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

    #[tokio::test]
    async fn fetch_uses_path_override_for_path_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my-crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        let result = RustCrateFetch::new("my-crate", &workspace, tmp.path())
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

        // Query with hyphen, workspace entry uses underscore
        let result = RustCrateFetch::new("my-crate", &workspace, tmp.path())
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my_crate");
        assert_eq!(result.path, tmp.path());
    }

    #[tokio::test]
    async fn fetch_skips_path_override_when_version_specified() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("serde", "1.0.210", Some(tmp.path().to_path_buf()))];

        // With an explicit version spec, path overrides are skipped
        // (falls through to registry resolution).
        let result = RustCrateFetch::new("serde", &workspace, tmp.path())
            .version("=99.99.99")
            .fetch()
            .await;

        assert!(
            result.is_err(),
            "should not use path override when version is specified"
        );
    }
}
