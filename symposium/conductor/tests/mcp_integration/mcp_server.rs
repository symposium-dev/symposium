//! Simple MCP server implementation for testing
//!
//! This module provides a simple MCP server that can be served over streams
//! (not a separate process). It implements a basic "echo" tool for testing.

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

/// Parameters for the echo tool
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Simple test MCP server with an echo tool
#[derive(Clone)]
pub struct TestMcpServer {
    tool_router: ToolRouter<TestMcpServer>,
}

impl TestMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl TestMcpServer {
    /// Echo tool - returns the input message
    #[tool(description = "Echoes back the input message")]
    async fn echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Echo: {}",
            params.message
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for TestMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "test-mcp-server".to_string(),
                version: "0.1.0".to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some("A simple test MCP server with an echo tool".to_string()),
        }
    }
}
