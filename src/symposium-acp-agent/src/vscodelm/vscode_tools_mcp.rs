//! Synthetic MCP server that exposes VS Code-provided tools to ACP agents.
//!
//! This module bridges VS Code's Language Model API tools to ACP agents by creating
//! an MCP server that:
//! 1. Advertises VS Code tools to the agent via `tools/list`
//! 2. Routes tool invocations back to VS Code via the session actor
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        Session Actor                                 │
//! │                                                                      │
//! │  ┌──────────────┐     tools_tx      ┌─────────────────────────────┐ │
//! │  │              │ ───────────────►  │                             │ │
//! │  │  Request     │                   │  VscodeToolsMcpServer       │ │
//! │  │  Handler     │  ◄───────────────  │  (rmcp ServerHandler)       │ │
//! │  │              │    invocation_rx  │                             │ │
//! │  └──────────────┘                   └─────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

use std::borrow::Cow;
use std::sync::Arc;

use futures::channel::{mpsc, oneshot};
use rmcp::model::{
    CallToolRequestParam, CallToolResult, ErrorCode, InitializeRequestParam, InitializeResult,
    ListToolsResult, PaginatedRequestParam, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{Peer, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use tokio::sync::RwLock;

/// A tool definition received from VS Code.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VscodeTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A tool invocation request sent to the session actor.
#[derive(Debug)]
pub struct ToolInvocation {
    pub name: String,
    pub arguments: Option<serde_json::Map<String, serde_json::Value>>,
    pub result_tx: oneshot::Sender<Result<CallToolResult, String>>,
}

/// Shared state for the MCP server.
struct VscodeToolsState {
    /// Current list of tools from VS Code
    tools: Vec<VscodeTool>,
    /// Peer handle for sending notifications (set on first request)
    peer: Option<Peer<RoleServer>>,
}

/// Synthetic MCP server that exposes VS Code tools to ACP agents.
#[derive(Clone)]
pub struct VscodeToolsMcpServer {
    state: Arc<RwLock<VscodeToolsState>>,
    invocation_tx: mpsc::UnboundedSender<ToolInvocation>,
}

impl VscodeToolsMcpServer {
    /// Create a new VS Code tools MCP server.
    ///
    /// Takes a sender for tool invocations that will be used when the agent calls a tool.
    pub fn new(invocation_tx: mpsc::UnboundedSender<ToolInvocation>) -> Self {
        Self {
            state: Arc::new(RwLock::new(VscodeToolsState {
                tools: Vec::new(),
                peer: None,
            })),
            invocation_tx,
        }
    }

    /// Get a handle that can be used to update tools from another task.
    pub fn tools_handle(&self) -> VscodeToolsHandle {
        VscodeToolsHandle {
            state: self.state.clone(),
        }
    }
}

/// Handle for updating tools from outside the MCP server.
#[derive(Clone)]
pub struct VscodeToolsHandle {
    state: Arc<RwLock<VscodeToolsState>>,
}

impl VscodeToolsHandle {
    /// Set the initial list of tools without sending a notification.
    /// Use this before the MCP server is advertised to avoid race conditions.
    pub async fn set_initial_tools(&self, tools: Vec<VscodeTool>) {
        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        tracing::debug!(
            tool_count = tools.len(),
            ?tool_names,
            "setting initial VS Code tools"
        );

        let mut state = self.state.write().await;
        state.tools = tools;
    }

    /// Update the list of available tools and notify the client if changed.
    pub async fn update_tools(&self, tools: Vec<VscodeTool>) {
        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        tracing::debug!(
            tool_count = tools.len(),
            ?tool_names,
            "updating VS Code tools"
        );

        let (changed, peer) = {
            let mut state = self.state.write().await;

            // Check if the tool list actually changed
            let changed = !tools_equal(&state.tools, &tools);
            if changed {
                state.tools = tools;
            }

            (changed, state.peer.clone())
        };

        // Only notify if tools actually changed
        if changed {
            if let Some(peer) = peer {
                if let Err(e) = peer.notify_tool_list_changed().await {
                    tracing::warn!(?e, "failed to notify tool list changed");
                }
            }
        }
    }
}

/// Check if two tool lists are equal (by name, since that's the identity).
fn tools_equal(a: &[VscodeTool], b: &[VscodeTool]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // Compare by name - tools are identified by name
    let a_names: std::collections::HashSet<_> = a.iter().map(|t| &t.name).collect();
    let b_names: std::collections::HashSet<_> = b.iter().map(|t| &t.name).collect();
    a_names == b_names
}

impl ServerHandler for VscodeToolsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            server_info: rmcp::model::Implementation {
                name: "symposium-vscode-tools".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some("VS Code-provided tools bridged to ACP".to_string()),
        }
    }

    async fn initialize(
        &self,
        request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        tracing::debug!(
            client_name = ?request.client_info.name,
            client_version = ?request.client_info.version,
            "MCP initialize called"
        );

        // Store the peer at initialization time so we can send notifications later
        {
            let mut state = self.state.write().await;
            state.peer = Some(context.peer.clone());
            tracing::debug!("stored peer handle at MCP initialization");
        }

        // Call the default implementation
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }

        let result = InitializeResult {
            protocol_version: rmcp::model::ProtocolVersion::LATEST,
            capabilities: self.get_info().capabilities,
            server_info: self.get_info().server_info,
            instructions: self.get_info().instructions,
        };

        tracing::debug!(?result.capabilities, "MCP initialize complete");
        Ok(result)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let state = self.state.read().await;

        let tools: Vec<Tool> = state
            .tools
            .iter()
            .map(|t| {
                let input_schema = match &t.input_schema {
                    serde_json::Value::Object(obj) => Arc::new(obj.clone()),
                    _ => Arc::new(serde_json::Map::new()),
                };
                Tool {
                    name: Cow::Owned(t.name.clone()),
                    title: None,
                    description: Some(Cow::Owned(t.description.clone())),
                    input_schema,
                    output_schema: None,
                    annotations: None,
                    icons: None,
                    meta: None,
                }
            })
            .collect();

        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        tracing::debug!(tool_count = tools.len(), ?tool_names, "list_tools called");

        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool_name = %request.name, ?request.arguments, "call_tool called");

        // Check if tool exists
        {
            let state = self.state.read().await;
            if !state.tools.iter().any(|t| t.name == request.name.as_ref()) {
                tracing::warn!(tool_name = %request.name, "tool not found");
                return Err(ErrorData::new(
                    ErrorCode::INVALID_PARAMS,
                    format!("tool '{}' not found", request.name),
                    None,
                ));
            }
        }

        // Create a oneshot channel for the result
        let (result_tx, result_rx) = oneshot::channel();

        // Send invocation to session actor
        let invocation = ToolInvocation {
            name: request.name.to_string(),
            arguments: request.arguments,
            result_tx,
        };

        self.invocation_tx
            .unbounded_send(invocation)
            .map_err(|_| ErrorData::internal_error("session actor unavailable", None))?;

        // Wait for result from session actor
        match result_rx.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(ErrorData::internal_error(error, None)),
            Err(_) => Err(ErrorData::internal_error("tool invocation cancelled", None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_update_and_list_tools() {
        let (invocation_tx, _invocation_rx) = mpsc::unbounded();
        let server = VscodeToolsMcpServer::new(invocation_tx);
        let handle = server.tools_handle();

        // Initially empty - check via internal state
        {
            let state = server.state.read().await;
            assert!(state.tools.is_empty());
        }

        // Update tools via handle
        handle
            .update_tools(vec![VscodeTool {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }])
            .await;

        // Now has one tool
        {
            let state = server.state.read().await;
            assert_eq!(state.tools.len(), 1);
            assert_eq!(state.tools[0].name, "test_tool");
        }
    }
}
