//! Agent component that verifies MCP server configuration and handles prompts

use agent_client_protocol::{
    self as acp, AgentCapabilities, ContentBlock, InitializeRequest, InitializeResponse, McpServer,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionNotification,
    SessionUpdate, StopReason, TextContent,
};
use conductor::component::{Cleanup, ComponentProvider};
use futures::{AsyncRead, AsyncWrite};
use scp::{JsonRpcConnection, JsonRpcConnectionCx, JsonRpcCxExt};
use std::pin::Pin;

pub struct AgentComponentProvider;

impl ComponentProvider for AgentComponentProvider {
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error> {
        cx.spawn(async move {
            JsonRpcConnection::new(outgoing_bytes, incoming_bytes)
                .name("agent-component")
                .on_receive_request(async move |request: InitializeRequest, request_cx| {
                    // Simple initialization response
                    let response = InitializeResponse {
                        protocol_version: request.protocol_version,
                        agent_capabilities: AgentCapabilities::default(),
                        auth_methods: vec![],
                        meta: None,
                    };
                    request_cx.respond(response)
                })
                .on_receive_request(async move |request: NewSessionRequest, request_cx| {
                    assert_eq!(request.mcp_servers.len(), 1);

                    // Although the proxy injects an HTTP server, it will be rewritten to stdio by the conductor.
                    let mcp_server = &request.mcp_servers[0];
                    assert!(
                        matches!(mcp_server, McpServer::Stdio { .. }),
                        "expected a stdio MCP server: {:?}",
                        request.mcp_servers
                    );

                    // Verify the stdio configuration is correct
                    if let McpServer::Stdio {
                        name,
                        command,
                        args,
                        ..
                    } = mcp_server
                    {
                        assert_eq!(name, "eg");
                        assert_eq!(command.to_str().unwrap(), "conductor");
                        assert_eq!(args.len(), 2);
                        assert_eq!(args[0], "mcp");
                        // args[1] is the port number, which varies
                        tracing::debug!(
                            port = %args[1],
                            "MCP server correctly configured: conductor mcp"
                        );
                    }

                    // Simple session response
                    let response = NewSessionResponse {
                        session_id: "test-session-123".into(),
                        modes: None,
                        meta: None,
                    };
                    request_cx.respond(response)
                })
                .on_receive_request(async move |request: PromptRequest, request_cx| {
                    tracing::debug!(
                        session_id = %request.session_id.0,
                        "Received prompt request"
                    );

                    // Send initial message
                    request_cx.send_notification(SessionNotification {
                        session_id: request.session_id.clone(),
                        update: SessionUpdate::AgentMessageChunk {
                            content: ContentBlock::Text(TextContent {
                                annotations: None,
                                text: "Hello. I will now use the MCP tool".to_string(),
                                meta: None,
                            }),
                        },
                        meta: None,
                    })?;

                    // End the turn
                    let response = PromptResponse {
                        stop_reason: StopReason::EndTurn,
                        meta: None,
                    };
                    request_cx.respond(response)
                })
                .serve()
                .await
        })?;

        Ok(Cleanup::None)
    }
}

pub fn create() -> Box<dyn ComponentProvider> {
    Box::new(AgentComponentProvider)
}
