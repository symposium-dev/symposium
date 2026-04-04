//! Cache management for extracted crates

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::extraction::CrateExtractor;

/// Manages access to cargo's registry cache and our extraction cache.
///
/// Looks up crate sources in three locations (in order): our extraction cache,
/// cargo's extracted sources, and cargo's `.crate` archive cache.
pub struct CacheManager {
    /// Path to `~/.cargo/registry/`, containing `src/` (extracted) and `cache/` (.crate files).
    cargo_registry_dir: PathBuf,
    /// Path to `~/.symposium/cache/extractions/`, where we extract crates ourselves.
    extraction_cache_dir: PathBuf,
}

impl CacheManager {
    /// Create a new cache manager.
    ///
    /// Fails if `CARGO_HOME` cannot be determined (e.g., `$HOME` is unset
    /// and no `CARGO_HOME` environment variable is provided).
    pub fn new(cache_dir: &std::path::Path) -> Result<Self> {
        let cargo_home = home::cargo_home().context("could not determine CARGO_HOME directory")?;

        let cargo_registry_dir = cargo_home.join("registry");

        let extraction_cache_dir = cache_dir.join("extractions");

        Ok(Self {
            cargo_registry_dir,
            extraction_cache_dir,
        })
    }

    /// Get or extract a crate, returning the path to the extracted source.
    ///
    /// Checks (in order): our extraction cache, cargo's `registry/src/`,
    /// cargo's `registry/cache/*.crate`. Falls back to downloading from crates.io.
    pub async fn get_or_extract_crate(
        &self,
        crate_name: &str,
        version: &str,
        extractor: &CrateExtractor,
    ) -> Result<PathBuf> {
        // 1. Check if already extracted in our cache
        let extraction_path = self
            .extraction_cache_dir
            .join(format!("{crate_name}-{version}"));
        if extraction_path.exists() {
            return Ok(extraction_path);
        }

        // 2. Check cargo's extracted sources
        if let Some(cargo_src_path) = self.find_cargo_extracted_crate(crate_name, version)? {
            return Ok(cargo_src_path);
        }

        // 3. Check cargo's .crate cache
        if let Some(cached_crate_path) = self.find_cached_crate(crate_name, version)? {
            return extractor
                .extract_crate_to_cache(&cached_crate_path, &extraction_path)
                .await;
        }

        // 4. Download and extract
        extractor
            .download_and_extract_crate(crate_name, version, &extraction_path)
            .await
    }

    /// Find an already-extracted crate in cargo's src cache (`~/.cargo/registry/src/`).
    fn find_cargo_extracted_crate(
        &self,
        crate_name: &str,
        version: &str,
    ) -> Result<Option<PathBuf>> {
        let src_dir = self.cargo_registry_dir.join("src");
        if !src_dir.exists() {
            return Ok(None);
        }

        let crate_dir_name = format!("{crate_name}-{version}");

        for entry in std::fs::read_dir(src_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("index.") {
                    let crate_path = entry.path().join(&crate_dir_name);
                    if crate_path.exists() {
                        return Ok(Some(crate_path));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Find a `.crate` archive in cargo's download cache (`~/.cargo/registry/cache/`).
    fn find_cached_crate(&self, crate_name: &str, version: &str) -> Result<Option<PathBuf>> {
        let cache_dir = self.cargo_registry_dir.join("cache");
        if !cache_dir.exists() {
            return Ok(None);
        }

        let crate_filename = format!("{crate_name}-{version}.crate");

        for entry in std::fs::read_dir(cache_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("index.") {
                    let crate_path = entry.path().join(&crate_filename);
                    if crate_path.exists() {
                        return Ok(Some(crate_path));
                    }
                }
            }
        }

        Ok(None)
    }
}
