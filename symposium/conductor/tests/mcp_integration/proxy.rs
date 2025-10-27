//! Proxy component that provides MCP tools

use agent_client_protocol::{
    self as acp, InitializeRequest, McpServer, NewSessionRequest, NewSessionResponse,
};
use conductor::component::{Cleanup, ComponentProvider};
use futures::{AsyncRead, AsyncWrite};
use scp::{JsonRpcConnection, JsonRpcConnectionCx, JsonRpcCxExt, MetaCapabilityExt, Proxy};
use std::pin::Pin;

pub struct ProxyComponentProvider;

impl ComponentProvider for ProxyComponentProvider {
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error> {
        cx.spawn(async move {
            JsonRpcConnection::new(outgoing_bytes, incoming_bytes)
                .name("proxy-component")
                .on_receive_request(async move |mut request: InitializeRequest, request_cx| {
                    // Remove proxy capability before forwarding
                    request = request.remove_meta_capability(Proxy);

                    // Forward to agent and add proxy capability to response
                    request_cx
                        .send_request_to_successor(request)
                        .await_when_response_received(async move |response| {
                            let mut response = response?;
                            response = response.add_meta_capability(Proxy);
                            request_cx.respond(response)
                        })
                })
                .on_receive_request(async move |mut request: NewSessionRequest, request_cx| {
                    request.mcp_servers.push(McpServer::Http {
                        name: "eg".to_string(),
                        url: format!("acp:eg"),
                        headers: vec![],
                    });

                    request_cx
                        .send_request_to_successor(request)
                        .await_when_response_received(
                            async move |response: Result<NewSessionResponse, acp::Error>| {
                                request_cx.respond(response?)
                            },
                        )
                })
                .serve()
                .await
        })?;

        Ok(Cleanup::None)
    }
}

pub fn create() -> Box<dyn ComponentProvider> {
    Box::new(ProxyComponentProvider)
}
