//! Agent component that echoes back prompts

use agent_client_protocol::{
    self as acp, AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse,
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
                    assert!(!request.mcp_servers.is_empty());

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
