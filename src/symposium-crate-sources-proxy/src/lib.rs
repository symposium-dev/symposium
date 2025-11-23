//! Rust Crate Sources Proxy
//!
//! An ACP proxy that provides the `rust_crate_query` MCP tool for researching
//! Rust crate sources via dedicated sub-agent sessions.

mod crate_sources_mcp;
mod eg;
mod research_agent;
mod state;

use anyhow::Result;
use sacp::component::Component;
use sacp_proxy::{AcpProxyExt, McpServiceRegistry};
use state::ResearchState;
use std::sync::Arc;

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

    CrateSourcesProxy.serve(sacp_tokio::Stdio::new()).await?;

    Ok(())
}

/// A proxy which forwards all messages to its successor, adding access to the rust-crate-query MCP server.
pub struct CrateSourcesProxy;

impl Component for CrateSourcesProxy {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        // Create shared state for tracking active research sessions
        let state = Arc::new(ResearchState::new());

        // Create MCP service registry with the user-facing service
        let mcp_registry = McpServiceRegistry::default().with_mcp_server(
            "rust-crate-query",
            research_agent::build_server(state.clone()),
        )?;

        sacp::JrHandlerChain::new()
            .name("rust-crate-sources-proxy")
            .provide_mcp(mcp_registry)
            .with_handler(research_agent::PermissionAutoApprover::new(state.clone()))
            .proxy()
            .connect_to(client)?
            .serve()
            .await
    }
}
