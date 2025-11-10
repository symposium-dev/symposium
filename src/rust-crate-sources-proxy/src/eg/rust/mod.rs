//! Rust-specific example searching functionality

use crate::eg::{Result, SearchResult};
use regex::Regex;

mod version;
mod cache;
mod extraction;
mod search;

pub use version::VersionResolver;
pub use cache::CacheManager;
pub use extraction::CrateExtractor;
pub use search::CrateSearcher;

/// Builder for searching Rust crate examples
pub struct RustCrateSearch {
    crate_name: String,
    version_spec: Option<String>,
    pattern: Option<Regex>,
    context_lines: usize,
}

impl RustCrateSearch {
    /// Create a new search for the given crate name
    pub fn new(name: &str) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
            pattern: None,
            context_lines: 2, // Default context
        }
    }

    /// Specify a version constraint (e.g., "^1.0", "=1.2.3")
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Specify a regex pattern to search for within the crate
    pub fn pattern(mut self, pattern: &str) -> Result<Self> {
        let regex = Regex::new(pattern)
            .map_err(|e| crate::eg::EgError::Other(format!("Invalid regex pattern: {}", e)))?;
        self.pattern = Some(regex);
        Ok(self)
    }

    /// Execute the search
    pub async fn search(self) -> Result<SearchResult> {
        // 1. Resolve version
        let resolver = VersionResolver::new();
        let version = resolver.resolve_version(&self.crate_name, self.version_spec.as_deref()).await?;

        // 2. Get or extract crate source
        let cache_manager = CacheManager::new()?;
        let extractor = CrateExtractor::new();
        
        let checkout_path = cache_manager.get_or_extract_crate(&self.crate_name, &version, &extractor).await?;

        // 3. Search the extracted crate
        let searcher = CrateSearcher::new();
        let (example_matches, other_matches) = if let Some(pattern) = &self.pattern {
            searcher.search_crate(&checkout_path, pattern, self.context_lines)?
        } else {
            // No pattern - just return empty matches but still provide checkout_path
            (Vec::new(), Vec::new())
        };

        Ok(SearchResult {
            version,
            checkout_path,
            example_matches,
            other_matches,
        })
    }
}
