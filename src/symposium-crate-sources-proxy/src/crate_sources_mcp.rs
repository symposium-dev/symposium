//! MCP service for research sub-agent sessions.
//!
//! Provides tools that research agents use to investigate Rust crate sources:
//! - `get_rust_crate_source`: Locates and extracts crate sources from crates.io
//! - `return_response_to_user`: Sends research findings back to complete the query
//!
//! This service is attached to NewSessionRequest when spawning research sessions.

use crate::eg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Parameters for the get_rust_crate_source tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetRustCrateSourceParams {
    /// Name of the crate to search
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Parameters for the return_response_to_user tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReturnResponseParams {
    /// The research findings to return to the user
    pub response: serde_json::Value,
}

/// Output from get_rust_crate_source tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct GetRustCrateSourceOutput {
    crate_name: String,
    version: String,
    checkout_path: String,
    message: String,
}

/// Output from return_response_to_user tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ReturnResponseOutput {
    message: String,
}

/// Build the MCP server for sub-agent research sessions.
///
/// Each instance is created for a specific research session and holds a channel
/// to send responses back to the waiting research agent.
pub fn build_server(
    response_tx: mpsc::Sender<serde_json::Value>,
) -> sacp::mcp_server::McpServer<sacp::ProxyToConductor> {
    use sacp::mcp_server::McpServer;

    McpServer::new()
        .instructions("Provides tools for researching Rust crate sources: get_rust_crate_source to locate crates, return_response_to_user to deliver findings")
        .tool_fn(
            "get_rust_crate_source",
            "Locate and extract Rust crate sources from crates.io. Returns the local path where the crate sources are available for reading.",
            async move |input: GetRustCrateSourceParams, _context| {
                let GetRustCrateSourceParams { crate_name, version } = input;

                tracing::debug!(
                    "Getting Rust crate source for '{}' version: {:?}",
                    crate_name,
                    version,
                );

                let mut search = eg::Eg::rust_crate(&crate_name);

                // Use version resolver for semver range support and project detection
                if let Some(version_spec) = version {
                    search = search.version(&version_spec);
                }

                let search_result = search.search().await.map_err(|e| {
                    anyhow::anyhow!("Search failed: {}", e)
                })?;

                let message = format!(
                    "Crate '{}' version {} extracted to {}",
                    crate_name,
                    search_result.version,
                    search_result.checkout_path.display()
                );

                Ok(GetRustCrateSourceOutput {
                    crate_name,
                    version: search_result.version.clone(),
                    checkout_path: search_result.checkout_path.display().to_string(),
                    message,
                })
            },
            |f, args, cx| Box::pin(f(args, cx)),
        )
        .tool_fn(
            "return_response_to_user",
            "Record the results that will be returned to the user. If invoked multiple times, the results will be appended to the previous response.",
            {
                let response_tx = response_tx.clone();
                async move |input: ReturnResponseParams, _context| {
                    let ReturnResponseParams { response } = input;

                    tracing::info!("Research complete, returning response");
                    tracing::debug!("Response: {}", response);

                    // Send the response through the channel to the waiting research agent
                    response_tx.send(response.clone()).await.map_err(|_| {
                        anyhow::anyhow!("Failed to send response: channel closed")
                    })?;

                    Ok(ReturnResponseOutput {
                        message: "Response delivered to waiting agent.".to_string(),
                    })
                }
            },
            |f, args, cx| Box::pin(f(args, cx)),
        )
}
