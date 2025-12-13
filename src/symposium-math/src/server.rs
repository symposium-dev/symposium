//! MCP server implementation using rmcp macros.

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo, ToolsCapability,
    },
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The Math MCP server.
#[derive(Clone)]
pub struct MathServer {
    tool_router: ToolRouter<MathServer>,
}

impl MathServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for MathServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Input for the average tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AverageInput {
    /// The numbers to average
    pub numbers: Vec<f64>,
}

/// Output from the average tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AverageOutput {
    /// The computed average
    pub average: f64,
    /// The count of numbers
    pub count: usize,
}

#[tool_router]
impl MathServer {
    #[tool(description = "Compute the average (arithmetic mean) of a list of numbers")]
    async fn average(
        &self,
        Parameters(input): Parameters<AverageInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if input.numbers.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "Cannot compute average of empty list",
                None,
            ));
        }

        let sum: f64 = input.numbers.iter().sum();
        let count = input.numbers.len();
        let average = sum / count as f64;

        let output = AverageOutput { average, count };
        Ok(CallToolResult::success(vec![
            Content::json(output).map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Failed to serialize output: {}", e), None)
            })?,
        ]))
    }
}

#[tool_handler]
impl ServerHandler for MathServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: "symposium-math".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some("A simple math server that can compute averages.".into()),
        }
    }
}

/// Run as standalone MCP server over stdio.
pub async fn run_mcp_stdio() -> Result<()> {
    let server = MathServer::new();
    let transport = rmcp::transport::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

/// Run as ACP proxy component that provides the MCP server.
pub async fn run_acp_proxy() -> Result<()> {
    use sacp::ProxyToConductor;
    use sacp::mcp_server::McpServiceRegistry;
    use sacp_rmcp::McpServiceRegistryRmcpExt;

    let mcp_registry =
        McpServiceRegistry::new().with_rmcp_server("symposium-math", MathServer::new)?;

    ProxyToConductor::builder()
        .name("symposium-math-proxy")
        .with_handler(mcp_registry)
        .serve(sacp_tokio::Stdio::new())
        .await?;

    Ok(())
}
