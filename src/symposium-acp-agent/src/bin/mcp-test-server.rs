use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct TestServer {
    tool_router: ToolRouter<Self>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct EchoRequest {
    text: String,
}

#[tool_router(router = tool_router)]
impl TestServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Echo the provided text.")]
    async fn echo(&self, params: Parameters<EchoRequest>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(params.0.text)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl rmcp::ServerHandler for TestServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = TestServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
