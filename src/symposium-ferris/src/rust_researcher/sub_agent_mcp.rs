//! MCP server for research sub-agent sessions.
//!
//! Provides tools that research agents use to investigate Rust crate sources:
//! - `get_rust_crate_source`: Locates and extracts crate sources from crates.io
//! - `return_response_to_user`: Sends research findings back to complete the query

use std::sync::{Arc, Mutex};

use sacp::{ProxyToConductor, mcp_server::McpServer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the return_response_to_user tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReturnResponseParams {
    /// The research findings to return to the user
    pub response: serde_json::Value,
}

/// Output from return_response_to_user tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ReturnResponseOutput {
    message: String,
}

/// Build the MCP server for sub-agent research sessions.
///
/// Each instance is created for a specific research session and holds a reference
/// to collect responses that will be returned to the calling agent.
pub fn build_server(
    responses: Arc<Mutex<Vec<serde_json::Value>>>,
) -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    let builder = McpServer::builder("ferris-research".to_string());

    let builder = crate::crate_sources::mcp::register(builder, true);

    let builder = builder
        .tool_fn_mut(
            "return_response_to_user",
            "Record research findings to return to the user. Can be invoked multiple times; all responses will be collected.",
            {
                let responses = responses.clone();
                move |input: ReturnResponseParams, _context| {
                    let responses = responses.clone();
                    async move {
                        let ReturnResponseParams { response } = input;

                        tracing::info!("Research complete, recording response");
                        tracing::debug!(response = %response, "Response content");

                        responses.lock().expect("not poisoned").push(response);

                        Ok(ReturnResponseOutput {
                            message: "Response recorded.".to_string(),
                        })
                    }
                }
            },
            sacp::tool_fn_mut!(),
        );

    builder.build()
}
