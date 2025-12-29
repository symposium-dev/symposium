//! MCP service for research sub-agent sessions.
//!
//! Provides tools that research agents use to investigate Rust crate sources:
//! - `get_rust_crate_source`: Locates and extracts crate sources from crates.io
//! - `return_response_to_user`: Sends research findings back to complete the query
//!
//! This service is attached to NewSessionRequest when spawning research sessions.

use std::sync::{Arc, Mutex};

use sacp::mcp_server::McpServer;
use sacp::ProxyToConductor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    responses: Arc<Mutex<Vec<serde_json::Value>>>,
) -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    McpServer::builder("rust-crate-sources".to_string())
        .instructions("Provides tools for researching Rust crate sources: get_rust_crate_source to locate crates, return_response_to_user to deliver findings")
        .tool_fn_mut(
            "get_rust_crate_source",
            "Locate and extract Rust crate sources from crates.io. Returns the local path where the crate sources are available for reading.",
            async move |input: GetRustCrateSourceParams, _context| {
                let GetRustCrateSourceParams { crate_name, version } = input;

                tracing::debug!(
                    "Getting Rust crate source for '{}' version: {:?}",
                    crate_name,
                    version,
                );

                let mut fetch = symposium_ferris::Ferris::rust_crate(&crate_name);

                // Use version resolver for semver range support and project detection
                if let Some(version_spec) = version {
                    fetch = fetch.version(&version_spec);
                }

                let result = fetch.fetch().await.map_err(|e| {
                    anyhow::anyhow!("Fetch failed: {}", e)
                })?;

                let message = format!(
                    "Crate '{}' version {} extracted to {}",
                    crate_name,
                    result.version,
                    result.path.display()
                );

                Ok(GetRustCrateSourceOutput {
                    crate_name,
                    version: result.version.clone(),
                    checkout_path: result.path.display().to_string(),
                    message,
                })
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "return_response_to_user",
            "Record the results that will be returned to the user. If invoked multiple times, the results will be appended to the previous response.",
            {
                let responses = responses.clone();
                move |input: ReturnResponseParams, _context| {
                    let responses = responses.clone();
                    async move {
                        let ReturnResponseParams { response } = input;

                        tracing::info!("Research complete, returning response");
                        tracing::debug!("Response: {}", response);

                        // Send the response through the channel to the waiting research agent
                        responses.lock().expect("not poisoned").push(response);

                        Ok(ReturnResponseOutput {
                            message: "Response delivered to waiting agent.".to_string(),
                        })
                    }
                }
            },
            sacp::tool_fn_mut!(),
        )
        .build()
}
