//! # symposium-ferris - Ferris MCP Server
//!
//! Helpful tools for Rust development, exposed as an MCP server.
//!
//! Currently provides:
//! - Crate source access: download, cache, and extract Rust crate source code

pub mod error;
pub mod rust;

pub use error::{FerrisError, Result};
pub use rust::FetchResult;

/// Main entry point for Ferris functionality
pub struct Ferris;

impl Ferris {
    /// Access a Rust crate's source code
    pub fn rust_crate(name: &str) -> rust::RustCrateFetch {
        rust::RustCrateFetch::new(name)
    }
}
