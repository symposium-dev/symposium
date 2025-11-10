//! Cache management for extracted crates

use crate::eg::{Result, EgError};
use std::path::PathBuf;

/// Manages access to cargo's cache and our extraction cache
pub struct CacheManager {
    cargo_cache_dir: PathBuf,
    extraction_cache_dir: PathBuf,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new() -> Result<Self> {
        let cargo_home = home::cargo_home()
            .map_err(EgError::CargoHomeNotFound)?;
        
        let cargo_cache_dir = cargo_home.join("registry");
        
        // Use platform-appropriate cache directory for our extractions
        let extraction_cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| cargo_home.clone())
            .join("eg")
            .join("extractions");
        
        Ok(Self { 
            cargo_cache_dir,
            extraction_cache_dir,
        })
    }

    /// Get or extract a crate, returning the path to the extracted source
    pub async fn get_or_extract_crate(
        &self,
        crate_name: &str,
        version: &str,
        extractor: &super::CrateExtractor,
    ) -> Result<PathBuf> {
        // 1. Check if already extracted in our cache
        let extraction_path = self.extraction_cache_dir.join(format!("{}-{}", crate_name, version));
        if extraction_path.exists() {
            return Ok(extraction_path);
        }

        // 2. Check cargo's extracted sources
        if let Some(cargo_src_path) = self.find_cargo_extracted_crate(crate_name, version)? {
            return Ok(cargo_src_path);
        }

        // 3. Check cargo's .crate cache
        if let Some(cached_crate_path) = self.find_cached_crate(crate_name, version)? {
            return extractor.extract_crate_to_cache(&cached_crate_path, &extraction_path).await;
        }

        // 4. Download and extract
        extractor.download_and_extract_crate(crate_name, version, &extraction_path).await
    }

    /// Find extracted crate in cargo's src cache
    fn find_cargo_extracted_crate(&self, crate_name: &str, version: &str) -> Result<Option<PathBuf>> {
        let src_dir = self.cargo_cache_dir.join("src");
        if !src_dir.exists() {
            return Ok(None);
        }

        let crate_dir_name = format!("{}-{}", crate_name, version);
        
        // Look for registry directories (e.g., index.crates.io-*)
        for entry in std::fs::read_dir(src_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let registry_name = entry.file_name();
                if registry_name.to_string_lossy().starts_with("index.") {
                    let crate_path = entry.path().join(&crate_dir_name);
                    if crate_path.exists() {
                        return Ok(Some(crate_path));
                    }
                }
            }
        }
        
        Ok(None)
    }

    /// Find a cached .crate file for the given crate and version
    fn find_cached_crate(&self, crate_name: &str, version: &str) -> Result<Option<PathBuf>> {
        let cache_dir = self.cargo_cache_dir.join("cache");
        if !cache_dir.exists() {
            return Ok(None);
        }

        let crate_filename = format!("{}-{}.crate", crate_name, version);
        
        // Look for registry directories
        for entry in std::fs::read_dir(cache_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let registry_name = entry.file_name();
                if registry_name.to_string_lossy().starts_with("index.") {
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
