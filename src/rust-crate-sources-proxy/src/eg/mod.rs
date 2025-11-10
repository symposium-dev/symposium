//! # eg - Example Search Library
//!
//! Programmatic access to library examples and documentation.

pub mod error;
pub mod rust;

pub use error::{EgError, Result};

use std::path::PathBuf;

/// Main entry point for example searches
pub struct Eg;

impl Eg {
    /// Search for examples in a Rust crate
    pub fn rust_crate(name: &str) -> rust::RustCrateSearch {
        rust::RustCrateSearch::new(name)
    }
}

/// Result of an example search
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    /// The exact version that was searched
    pub version: String,
    /// Path to the full crate extraction on disk
    pub checkout_path: PathBuf,
    /// Matches found in examples/ directory
    pub example_matches: Vec<Match>,
    /// Matches found elsewhere in the crate
    pub other_matches: Vec<Match>,
}

/// A search match with context
#[derive(Debug, Clone, serde::Serialize)]
pub struct Match {
    /// Relative path within the crate
    pub file_path: PathBuf,
    /// 1-based line number where match was found
    pub line_number: u32,
    /// The line containing the match
    pub line_content: String,
    /// Lines before the match for context
    pub context_before: Vec<String>,
    /// Lines after the match for context
    pub context_after: Vec<String>,
}
