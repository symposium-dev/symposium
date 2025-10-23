//! Mock agent component that uses rmcp to invoke MCP tools

use agent_client_protocol::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse,
};
use conductor::component::MockComponentImpl;
use rmcp::{ClientHandler, Peer, RoleClient, ServiceExt, model::*};
use scp::{AcpClientToAgentCallbacks, AcpClientToAgentMessages, JsonRpcCx, JsonRpcRequestCx};
use tokio::process::Command;
use tracing::Instrument;

/// Callbacks for the mock agent component
struct AgentCallbacks {
    /// Connection to the MCP bridge via rmcp
    mcp_peer: Option<Peer<RoleClient>>,
}

impl AcpClientToAgentCallbacks for AgentCallbacks {
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Agent: received initialize");

        // Agent should NOT receive proxy capability (it's the last component)
        let has_proxy_capability = args
            .meta
            .as_ref()
            .and_then(|m| m.get("symposium"))
            .and_then(|s| s.get("proxy"))
            .and_then(|p| p.as_bool())
            .unwrap_or(false);

        assert!(
            !has_proxy_capability,
            "Agent should not receive proxy capability"
        );

        // Agent does NOT support mcp_acp_transport
        let _ = response.respond(InitializeResponse {
            protocol_version: Default::default(),
            agent_capabilities: Default::default(),
            auth_methods: vec![],
            meta: None, // No mcp_acp_transport capability
        });

        Ok(())
    }

    async fn authenticate(
        &mut self,
        _args: agent_client_protocol::AuthenticateRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn session_cancel(
        &mut self,
        _args: agent_client_protocol::CancelNotification,
        _cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn new_session(
        &mut self,
        args: NewSessionRequest,
        response: JsonRpcRequestCx<NewSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Agent: received new_session");
        tracing::info!("Agent: MCP servers = {:?}", args.mcp_servers);

        // Agent should receive modified MCP server list with stdio transport
        // pointing to "conductor mcp $PORT"

        // Extract MCP server info from the first server
        if let Some(mcp_server) = args.mcp_servers.first() {
            if let agent_client_protocol::McpServer::Stdio {
                command,
                args: cmd_args,
                ..
            } = mcp_server
            {
                tracing::info!("Agent: Spawning MCP bridge: {} {:?}", command, cmd_args);

                // Spawn the bridge process
                let mut child = Command::new(command)
                    .args(cmd_args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .expect("Failed to spawn MCP bridge");

                let stdin = child.stdin.take().unwrap();
                let stdout = child.stdout.take().unwrap();

                // Give the bridge a moment to start
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                // Create rmcp client
                #[derive(Clone)]
                struct MockClient;

                impl ClientHandler for MockClient {
                    fn get_info(&self) -> ClientInfo {
                        ClientInfo {
                            protocol_version: ProtocolVersion::default(),
                            capabilities: ClientCapabilities::default(),
                            client_info: Implementation {
                                name: "test-agent".to_string(),
                                version: "1.0.0".to_string(),
                            },
                        }
                    }
                }

                let client = MockClient;
                let running = client
                    .serve((stdout, stdin))
                    .await
                    .expect("Failed to start rmcp client");

                // Store the peer for later tool invocation
                self.mcp_peer = Some(running.peer());

                // Spawn a task to keep the service running
                tokio::spawn(async move {
                    let _ = running.waiting().await;
                    let _ = child.kill().await;
                });

                tracing::info!("Agent: Successfully connected to MCP bridge via rmcp");
            }
        }

        let _ = response.respond(NewSessionResponse {
            session_id: "agent-session-456".to_string().into(),
            modes: Default::default(),
            meta: None,
        });

        Ok(())
    }

    async fn load_session(
        &mut self,
        _args: agent_client_protocol::LoadSessionRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn prompt(
        &mut self,
        _args: PromptRequest,
        response: JsonRpcRequestCx<PromptResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Agent: received prompt");

        // Use rmcp to invoke go_go_gadget_shoes tool
        if let Some(peer) = &self.mcp_peer {
            tracing::info!("Agent: Invoking go_go_gadget_shoes tool via rmcp");

            let tool_result = peer
                .call_tool(CallToolRequestParam {
                    name: "go_go_gadget_shoes".to_string(),
                    arguments: Some(serde_json::json!({})),
                })
                .await
                .expect("Failed to call tool");

            tracing::info!("Agent: Tool result: {:?}", tool_result);

            assert!(
                !tool_result.is_error.unwrap_or(false),
                "Tool invocation should not error"
            );
        } else {
            tracing::warn!("Agent: No MCP peer available to invoke tool");
        }

        let _ = response.respond(PromptResponse {
            stop_reason: agent_client_protocol::StopReason::EndTurn,
            meta: None,
        });

        Ok(())
    }

    async fn set_session_mode(
        &mut self,
        _args: agent_client_protocol::SetSessionModeRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }
}

/// Create a mock agent component that uses rmcp to invoke MCP tools
pub fn create_mock_agent() -> MockComponentImpl {
    MockComponentImpl::new(move |connection| async move {
        let callbacks = AgentCallbacks { mcp_peer: None };

        let _ = connection
            .on_receive(AcpClientToAgentMessages::callback(callbacks))
            .serve()
            .instrument(tracing::info_span!("actor", id = "mock_agent"))
            .await;
    })
}
