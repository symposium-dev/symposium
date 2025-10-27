//! Agent component that verifies MCP server configuration

use agent_client_protocol::{
    self as acp, AgentCapabilities, InitializeRequest, InitializeResponse, McpServer,
    NewSessionRequest, NewSessionResponse,
};
use conductor::component::{Cleanup, ComponentProvider};
use futures::{AsyncRead, AsyncWrite};
use scp::{JsonRpcConnection, JsonRpcConnectionCx};
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
                        println!(
                            "✓ MCP server correctly configured: conductor mcp {}",
                            args[1]
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
                .serve()
                .await
        })?;

        Ok(Cleanup::None)
    }
}

pub fn create() -> Box<dyn ComponentProvider> {
    Box::new(AgentComponentProvider)
}
