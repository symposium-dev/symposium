//! Crate extraction to local cache

use crate::eg::{Result, EgError};
use flate2::read::GzDecoder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Handles extraction of .crate files to local cache
pub struct CrateExtractor;

impl CrateExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Extract a cached .crate file to the extraction cache
    pub async fn extract_crate_to_cache(
        &self,
        crate_path: &Path,
        extraction_path: &PathBuf,
    ) -> Result<PathBuf> {
        let file = fs::File::open(crate_path)?;
        self.extract_from_reader(file, extraction_path).await?;
        Ok(extraction_path.clone())
    }

    /// Download and extract a crate to the extraction cache
    pub async fn download_and_extract_crate(
        &self,
        crate_name: &str,
        version: &str,
        extraction_path: &PathBuf,
    ) -> Result<PathBuf> {
        let download_url = format!(
            "https://static.crates.io/crates/{}/{}-{}.crate",
            crate_name, crate_name, version
        );

        let response = reqwest::get(&download_url).await?;
        if !response.status().is_success() {
            return Err(EgError::Other(format!(
                "Failed to download crate: HTTP {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await?;
        self.extract_from_reader(std::io::Cursor::new(bytes), extraction_path).await?;
        Ok(extraction_path.clone())
    }

    /// Extract from any reader to the specified directory
    async fn extract_from_reader<R: Read>(
        &self,
        reader: R,
        extraction_path: &PathBuf,
    ) -> Result<()> {
        // Create extraction directory
        fs::create_dir_all(extraction_path)?;

        let gz_decoder = GzDecoder::new(reader);
        let mut archive = Archive::new(gz_decoder);

        // Extract all files
        archive.unpack(extraction_path)
            .map_err(|e| EgError::ExtractionError(format!("Failed to extract archive: {}", e)))?;

        // The archive typically contains a single directory with the crate name-version
        // We want to flatten this structure
        self.flatten_extraction(extraction_path)?;

        Ok(())
    }

    /// Flatten the extraction if it contains a single top-level directory
    fn flatten_extraction(&self, extraction_path: &PathBuf) -> Result<()> {
        let entries: Vec<_> = fs::read_dir(extraction_path)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // If there's exactly one entry and it's a directory, move its contents up
        if entries.len() == 1 {
            let entry = &entries[0];
            if entry.file_type()?.is_dir() {
                let inner_dir = entry.path();
                
                // Move all contents from inner directory to extraction_path
                for inner_entry in fs::read_dir(&inner_dir)? {
                    let inner_entry = inner_entry?;
                    let src = inner_entry.path();
                    let dst = extraction_path.join(inner_entry.file_name());
                    
                    if src.is_dir() {
                        self.move_dir(&src, &dst)?;
                    } else {
                        fs::rename(&src, &dst)?;
                    }
                }
                
                // Remove the now-empty inner directory
                fs::remove_dir(&inner_dir)?;
            }
        }

        Ok(())
    }

    /// Recursively move a directory
    fn move_dir(&self, src: &Path, dst: &Path) -> Result<()> {
        fs::create_dir_all(dst)?;
        
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            
            if src_path.is_dir() {
                self.move_dir(&src_path, &dst_path)?;
            } else {
                fs::rename(&src_path, &dst_path)?;
            }
        }
        
        fs::remove_dir(src)?;
        Ok(())
    }
}
