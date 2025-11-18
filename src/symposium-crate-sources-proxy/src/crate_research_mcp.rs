//! User-facing MCP service for researching Rust crates.
//!
//! Provides the `rust_crate_query` tool which allows agents to request research
//! about Rust crate source code by describing what information they need.
//! The service coordinates with research_agent to spawn sub-sessions that
//! investigate crate sources and return synthesized findings.

use crate::{crate_sources_mcp, research_agent, state::ResearchState};
use sacp::schema::{NewSessionResponse, PromptRequest, PromptResponse};
use sacp_proxy::McpServiceRegistry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{pin::pin, sync::Arc};
use tokio::sync::mpsc;

/// Parameters for the rust_crate_query tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustCrateQueryParams {
    /// Name of the Rust crate to research
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    /// Defaults to latest version if not specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_version: Option<String>,
    /// Research prompt describing what information you need about the crate.
    /// Examples:
    /// - "How do I use the derive macro for custom field names?"
    /// - "What are the signatures of all methods on tokio::runtime::Runtime?"
    /// - "Show me an example of using async-trait with associated types"
    pub prompt: String,
}

/// Output from the rust_crate_query tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct RustCrateQueryOutput {
    /// The research findings
    result: serde_json::Value,
}

/// Build the MCP server for crate research queries
pub fn build_server(state: Arc<ResearchState>) -> sacp_proxy::McpServer {
    use sacp_proxy::McpServer;

    McpServer::new()
        .instructions("Provides research capabilities for Rust crate source code via dedicated sub-agent sessions")
        .tool_fn(
            "rust_crate_query",
            "Research a Rust crate's source code. Provide the crate name and describe what you want to know. A specialized research agent will examine the crate sources and return findings.",
            {
                async move |input: RustCrateQueryParams, mcp_cx| {
                    let RustCrateQueryParams {
                        crate_name,
                        crate_version,
                        prompt,
                    } = input;

                    tracing::info!(
                        "Received crate query for '{}' version: {:?}",
                        crate_name,
                        crate_version
                    );
                    tracing::debug!("Research prompt: {}", prompt);

                    let cx = mcp_cx.connection_cx();

                    // Create a channel for receiving responses from the sub-agent's return_response_to_user calls
                    let (response_tx, mut response_rx) = mpsc::channel::<serde_json::Value>(32);

                    // Create a fresh MCP service registry for this research session
                    let sub_agent_mcp_registry = McpServiceRegistry::default()
                        .with_mcp_server(
                            "rust-crate-sources",
                            crate_sources_mcp::build_server(response_tx.clone()),
                        )
                        .map_err(|e| anyhow::anyhow!("Failed to create MCP registry: {}", e))?;

                    // Spawn the sub-agent session with the per-instance MCP registry
                    let NewSessionResponse {
                        session_id,
                        modes: _,
                        meta: _,
                    } = cx
                        .send_request(research_agent::research_agent_session_request(
                            sub_agent_mcp_registry,
                        ).map_err(|e| anyhow::anyhow!("Failed to create session request: {}", e))?)
                        .block_task()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to spawn research session: {}", e))?;

                    tracing::info!("Research session created: {}", session_id);

                    // Register this session_id in shared state so permission requests are auto-approved
                    state.register_session(&session_id);

                    let mut responses = vec![];
                    let (result, _) = futures::future::select(
                        // Collect responses from the response channel
                        pin!(async {
                            while let Some(response) = response_rx.recv().await {
                                responses.push(response);
                            }
                            Ok::<(), anyhow::Error>(())
                        }),
                        pin!(async {
                            let research_prompt = research_agent::build_research_prompt(&prompt);
                            let prompt_request = PromptRequest {
                                session_id: session_id.clone(),
                                prompt: vec![research_prompt.into()],
                                meta: None,
                            };

                            let PromptResponse {
                                stop_reason,
                                meta: _,
                            } = cx
                                .send_request(prompt_request)
                                .block_task()
                                .await
                                .map_err(|e| anyhow::anyhow!("Prompt request failed: {}", e))?;

                            tracing::info!(
                                "Research complete for session {session_id} ({stop_reason:?})"
                            );

                            Ok::<(), anyhow::Error>(())
                        }),
                    )
                    .await
                    .factor_first();
                    result?;

                    // Unregister the session now that research is complete
                    state.unregister_session(&session_id);

                    // Return the accumulated responses
                    let response = if responses.len() == 1 {
                        responses.pop().expect("singleton")
                    } else {
                        serde_json::Value::Array(responses)
                    };

                    tracing::info!("Research complete for '{}'", crate_name);

                    Ok(RustCrateQueryOutput { result: response })
                }
            },
            |f, args, cx| Box::pin(f(args, cx)),
        )
}
