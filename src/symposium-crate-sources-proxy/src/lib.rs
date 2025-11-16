//! Rust Crate Sources Proxy
//!
//! An ACP proxy that provides the `rust_crate_query` MCP tool for researching
//! Rust crate sources via dedicated sub-agent sessions.

mod crate_research_mcp;
mod crate_sources_mcp;
mod eg;
mod research_agent;

use anyhow::Result;
use fxhash::FxHashSet;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use sacp::component::Component;
use sacp_proxy::{AcpProxyExt, McpServiceRegistry};
use sacp_rmcp::McpServiceRegistryRmcpExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
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

/// A proxy which forwards all messages to its successor, adding access to the rust-crate-query MCP server.
pub struct CrateSourcesProxy;

impl Component for CrateSourcesProxy {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        // Create channel for research requests
        let (research_tx, mut research_rx) =
            mpsc::channel::<crate_research_mcp::ResearchRequest>(32);

        // Create MCP service registry with the user-facing service
        let research_tx_clone = research_tx.clone();
        let mcp_registry = McpServiceRegistry::default()
            .with_rmcp_server("rust-crate-query", move || {
                crate_research_mcp::CrateQueryService::new(research_tx_clone.clone())
            })?;

        // MCP registry for research sub-agent sessions.
        // This registry is cloned and attached to each NewSessionRequest created by handle_research_request.
        // It provides the tools the sub-agent needs: get_rust_crate_source and return_response_to_user.
        let research_agent_mcp_registry = McpServiceRegistry::default()
            .with_rmcp_server("rust-crate-sources", RustCrateSourcesService::new)?;

        // Create shared state for tracking active research sessions
        let state = Arc::new(ResearchState {
            active_research_session_ids: Mutex::new(FxHashSet::default()),
        });

        sacp::JrHandlerChain::new()
            .name("rust-crate-sources-proxy")
            .provide_mcp(mcp_registry)
            .with_spawned(|cx| async move {
                tracing::info!("Research request handler started");

                while let Some(request) = research_rx.recv().await {
                    cx.spawn({
                        let cx = cx.clone();
                        let state = state.clone();
                        let registry = research_agent_mcp_registry.clone();
                        async move { research_agent::run(cx, state, registry, request).await }
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

/// Shared state tracking active research sessions.
///
/// This state is shared between:
/// - The main event loop (in Component::serve) which uses it to identify research sessions
///   when handling RequestPermissionRequest, tool calls, etc.
/// - The handle_research_request functions which register/unregister session_ids
///
/// Note: The oneshot::Sender for sending responses back is NOT stored here.
/// It's owned by the handle_research_request function and used directly when
/// return_response_to_user is called.
pub struct ResearchState {
    /// Set of session IDs that correspond to active research requests.
    /// The main loop checks this to decide how to handle session-specific messages.
    pub active_research_session_ids: Mutex<FxHashSet<String>>,
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
