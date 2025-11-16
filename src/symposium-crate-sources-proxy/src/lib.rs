//! Rust Crate Sources Proxy
//!
//! An ACP proxy that provides the `rust_crate_query` MCP tool for researching
//! Rust crate sources via dedicated sub-agent sessions.

pub mod eg;
pub mod sub_agent;
pub mod user_facing;

use anyhow::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use sacp::{component::Component, JrConnectionCx};
use sacp_proxy::{AcpProxyExt, McpServiceRegistry};
use sacp_rmcp::McpServiceRegistryRmcpExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Run the proxy as a standalone binary connected to stdio
pub async fn run() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting rust-crate-sources-proxy");

    CrateSourcesProxy.serve(sacp_tokio::Stdio).await?;

    Ok(())
}

/// Handle a single research request by spawning a sub-agent session
async fn handle_research_request(
    cx: JrConnectionCx,
    request: user_facing::ResearchRequest,
) -> Result<(), sacp::Error> {
    tracing::info!(
        "Handling research request for crate '{}' version {:?}",
        request.crate_name,
        request.crate_version
    );

    // TODO: Implementation steps:
    // 1. Send NewSessionRequest with sub-agent MCP server
    // 2. Get session_id back
    // 3. Store session_id → request.response_tx in shared state
    // 4. Send PromptRequest(session_id, request.prompt)
    // 5. Wait for sub-agent to call return_response_to_user

    // Placeholder: immediately send a response
    let placeholder_response = format!(
        "Research request received for '{}'. Session spawning not yet implemented.",
        request.crate_name
    );

    request
        .response_tx
        .send(placeholder_response)
        .map_err(|_| sacp::Error::internal_error())?;

    Ok(())
}

/// A proxy which forwards all messages to its successor, adding access to the rust-crate-query MCP server.
pub struct CrateSourcesProxy;

impl Component for CrateSourcesProxy {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        // Create channel for research requests
        let (research_tx, mut research_rx) = mpsc::channel::<user_facing::ResearchRequest>(32);

        // Create MCP service registry with the user-facing service
        let research_tx_clone = research_tx.clone();
        let mcp_registry = McpServiceRegistry::default()
            .with_rmcp_server("rust-crate-query", move || {
                user_facing::CrateQueryService::new(research_tx_clone.clone())
            })?;

        sacp::JrHandlerChain::new()
            .name("rust-crate-sources-proxy")
            .provide_mcp(mcp_registry)
            .with_spawned(|cx| async move {
                tracing::info!("Research request handler started");

                while let Some(request) = research_rx.recv().await {
                    cx.spawn({
                        let cx = cx.clone();
                        async move { handle_research_request(cx, request).await }
                    })?;
                }

                tracing::info!("Research request handler shutting down");
                Ok(())
            })
            .proxy()
            .connect_to(client)?
            .serve()
            .await
    }
}

/// Parameters for the get_rust_crate_source tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetRustCrateSourceParams {
    /// Name of the crate to search
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional search pattern (regex)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

/// MCP service that provides Rust crate source searching
#[derive(Clone)]
pub struct RustCrateSourcesService {
    tool_router: ToolRouter<RustCrateSourcesService>,
}

impl RustCrateSourcesService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl RustCrateSourcesService {
    /// Get Rust crate source with optional pattern search
    #[tool(
        description = "Get Rust crate source with optional pattern search. Always returns the source path, and optionally performs pattern matching if a search pattern is provided."
    )]
    async fn get_rust_crate_source(
        &self,
        Parameters(GetRustCrateSourceParams {
            crate_name,
            version,
            pattern,
        }): Parameters<GetRustCrateSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(
            "Getting Rust crate source for '{}' version: {:?} pattern: {:?}",
            crate_name,
            version,
            pattern
        );

        let has_pattern = pattern.is_some();
        let mut search = eg::Eg::rust_crate(&crate_name);

        // Use version resolver for semver range support and project detection
        if let Some(version_spec) = version {
            search = search.version(&version_spec);
        }

        if let Some(pattern) = pattern {
            search = search.pattern(&pattern).map_err(|e| {
                let error_msg = format!("Invalid regex pattern: {}", e);
                McpError::invalid_params(error_msg, None)
            })?;
        }

        let search_result = search.search().await.map_err(|e| {
            let error_msg = format!("Search failed: {}", e);
            McpError::internal_error(error_msg, None)
        })?;

        // Format the result
        let mut result = serde_json::json!({
            "crate_name": crate_name,
            "version": search_result.version,
            "checkout_path": search_result.checkout_path.display().to_string(),
            "message": format!(
                "Crate '{}' version {} extracted to {}",
                crate_name,
                search_result.version,
                search_result.checkout_path.display()
            ),
        });

        if has_pattern {
            result["example_matches"] = serde_json::to_value(&search_result.example_matches)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            result["other_matches"] = serde_json::to_value(&search_result.other_matches)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        } else {
            result["example_matches"] = serde_json::Value::Null;
            result["other_matches"] = serde_json::Value::Null;
        }

        let content_text = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(content_text)]))
    }
}

#[tool_handler]
impl ServerHandler for RustCrateSourcesService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rust-crate-sources".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some(
                "Provides access to Rust crate sources from crates.io with optional pattern search"
                    .to_string(),
            ),
        }
    }
}
