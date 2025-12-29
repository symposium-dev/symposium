//! Rust-specific crate source functionality

use std::path::PathBuf;

use crate::Result;

mod cache;
mod extraction;
pub(crate) mod mcp;
mod version;

pub use cache::CacheManager;
pub use extraction::CrateExtractor;
pub use version::VersionResolver;

/// Result of fetching a crate's sources
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The exact version that was fetched
    pub version: String,
    /// Path to the extracted crate sources on disk
    pub path: PathBuf,
}

/// Builder for accessing Rust crate source code
pub struct RustCrateFetch {
    crate_name: String,
    version_spec: Option<String>,
}

impl RustCrateFetch {
    /// Create a new fetch request for the given crate name
    pub fn new(name: &str) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
        }
    }

    /// Specify a version constraint (e.g., "^1.0", "=1.2.3")
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Fetch the crate sources, returning the path to extracted sources
    pub async fn fetch(self) -> Result<FetchResult> {
        // 1. Resolve version
        let resolver = VersionResolver::new();
        let version = resolver
            .resolve_version(&self.crate_name, self.version_spec.as_deref())
            .await?;

        // 2. Get or extract crate source
        let cache_manager = CacheManager::new()?;
        let extractor = CrateExtractor::new();

        let path = cache_manager
            .get_or_extract_crate(&self.crate_name, &version, &extractor)
            .await?;

        Ok(FetchResult { version, path })
    }
}
