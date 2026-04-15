//! Rust crate source fetching and management

use std::path::{Path, PathBuf};

use anyhow::Result;

mod cache;
mod extraction;
mod list;
mod version;

pub use list::workspace_semver_pairs;

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
    workspace: &'a [(String, semver::Version)],
    cache_dir: &'a Path,
}

impl<'a> RustCrateFetch<'a> {
    /// Create a new fetch request for the given crate name
    pub fn new(
        name: &str,
        workspace: &'a [(String, semver::Version)],
        cache_dir: &'a Path,
    ) -> Self {
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
        let resolver = version::VersionResolver::new(self.workspace);
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
