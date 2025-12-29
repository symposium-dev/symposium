//! MCP server implementation for Ferris tools.

use std::path::PathBuf;

use sacp::{ProxyToConductor, mcp_server::McpServer};

use crate::Ferris;

/// Build an MCP server with the configured Ferris tools.
pub fn build_server(
    config: Ferris,
    _cwd: PathBuf,
) -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    // Start with the base builder, then unconditionally register tools
    // Each tool internally checks if it's enabled

    let builder = McpServer::builder("ferris".to_string()).instructions(indoc::indoc! {"
        Rust development tools provided by Ferris.

        Available tools help with:
        - Fetching Rust crate source code for inspection
        - Researching Rust crate APIs and usage patterns
    "});

    // Register tools - each takes an `enabled` flag
    let builder = crate::crate_sources::mcp::register(builder, config.crate_sources);
    let builder = crate::rust_researcher::register(builder, config.rust_researcher);

    builder.build()
}
