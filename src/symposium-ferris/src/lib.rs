//! # symposium-ferris - Ferris MCP Server
//!
//! Helpful tools for Rust development, exposed as an MCP server.
//!
//! ## Tools
//!
//! - `crate_source`: Fetch and extract Rust crate source code by name and version
//! - `rust_researcher`: Research Rust crates using an LLM sub-agent (requires ACP session)
//!
//! ## Usage
//!
//! As a library for direct crate source access:
//! ```ignore
//! let result = Ferris::rust_crate("tokio").version("1.0").fetch().await?;
//! println!("Sources at: {}", result.path.display());
//! ```
//!
//! As an MCP server configuration:
//! ```ignore
//! let server = Ferris::default()
//!     .rust_researcher(true)
//!     .into_mcp_server(cwd);
//! ```

use std::path::Path;

use sacp::{ProxyToConductor, mcp_server::McpServer};

mod component;
mod crate_sources;
pub mod error;
mod mcp;
mod rust_researcher;

pub use component::FerrisComponent;
pub use crate_sources::{FetchResult, RustCrateFetch};
pub use error::{FerrisError, Result};

/// Ferris - Rust development tools
///
/// This struct serves two purposes:
/// 1. Configuration for the MCP server (which tools to enable)
/// 2. Entry point for the public Rust API (associated functions)
#[derive(Debug, Clone)]
pub struct Ferris {
    /// Enable the crate_source tool (default: true)
    pub crate_sources: bool,
    /// Enable the rust_researcher tool (default: false)
    pub rust_researcher: bool,
}

impl Default for Ferris {
    fn default() -> Self {
        Self {
            crate_sources: true,
            rust_researcher: false,
        }
    }
}

impl Ferris {
    /// Create a new Ferris configuration with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable the crate_source tool
    pub fn crate_sources(mut self, enabled: bool) -> Self {
        self.crate_sources = enabled;
        self
    }

    /// Enable or disable the rust_researcher tool
    pub fn rust_researcher(mut self, enabled: bool) -> Self {
        self.rust_researcher = enabled;
        self
    }

    /// Build an MCP server with the configured tools
    ///
    /// The `cwd` parameter specifies the working directory for the session.
    /// This may be used by tools that need workspace context.
    pub fn into_mcp_server(
        self,
        cwd: impl AsRef<Path>,
    ) -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
        mcp::build_server(self, cwd.as_ref().to_path_buf())
    }

    // -------------------------------------------------------------------------
    // Public API - associated functions for direct usage
    // -------------------------------------------------------------------------

    /// Access a Rust crate's source code
    ///
    /// Returns a builder that can be used to specify version constraints
    /// and fetch the crate sources.
    ///
    /// # Example
    /// ```ignore
    /// let result = Ferris::rust_crate("serde")
    ///     .version("1.0")
    ///     .fetch()
    ///     .await?;
    /// ```
    pub fn rust_crate(name: &str) -> RustCrateFetch {
        RustCrateFetch::new(name)
    }
}
