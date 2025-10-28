//! Proxy component that provides MCP tools

use agent_client_protocol::{
    self as acp, InitializeRequest, McpServer, NewSessionRequest, NewSessionResponse,
    PromptRequest, PromptResponse,
};
use conductor::component::{Cleanup, ComponentProvider};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};
use rmcp::ServiceExt;
use scp::{
    JsonRpcConnection, JsonRpcConnectionCx, JsonRpcConnectionExt, JsonRpcCxExt, JsonRpcRequestCx,
    McpConnectRequest, McpConnectResponse, McpOverAcpNotification, McpOverAcpRequest,
    MetaCapabilityExt, ProxiedMessage, Proxy, UntypedMessage,
};
use std::sync::Mutex;
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub struct ProxyComponentProvider;

/// Shared state for MCP connections
#[derive(Clone)]
struct McpConnectionState {
    /// Map of connection_id to channel for sending messages to MCP server
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<ProxiedMessage>>>>,
}

impl ComponentProvider for ProxyComponentProvider {
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error> {
        let state = McpConnectionState {
            connections: Arc::new(Mutex::new(HashMap::new())),
        };

        cx.spawn({
            let state = state.clone();
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
                .on_receive_request(async move |request: PromptRequest, request_cx| {
                    // Forward prompt requests to the agent
                    request_cx
                        .send_request_to_successor(request)
                        .await_when_response_received(
                            async move |response: Result<PromptResponse, acp::Error>| {
                                request_cx.respond(response?)
                            },
                        )
                })
                .on_receive_request_from_successor({
                    let state = state.clone();
                    async move |request: McpConnectRequest, request_cx| {
                        state.start_connection(request, request_cx).await
                    }
                })
                .on_receive_request_from_successor({
                    let state = state.clone();
                    async move |request: McpOverAcpRequest<UntypedMessage>, request_cx| {
                        state
                            .send_proxied_message(
                                request.connection_id,
                                ProxiedMessage::Request(request.request, request_cx),
                            )
                            .await
                    }
                })
                .on_receive_notification_from_successor({
                    let state = state.clone();
                    async move |notification: McpOverAcpNotification<UntypedMessage>, _| {
                        state
                            .send_proxied_message(
                                notification.connection_id,
                                ProxiedMessage::Notification(notification.notification),
                            )
                            .await
                    }
                })
                // All other notifications -- pass along to the predecessor
                .on_receive_request_from_successor(
                    async move |request: UntypedMessage, request_cx| {
                        tracing::debug!(?request, "on_receive_request_from_successor");
                        request_cx
                            .send_request(request)
                            .forward_to_request_cx(request_cx)
                    },
                )
                // All other notifications -- pass along to the predecessor
                .on_receive_notification_from_successor(
                    async move |notification: UntypedMessage, cx| {
                        tracing::debug!(?notification, "on_receive_request_from_successor");
                        cx.send_notification(notification)
                    },
                )
                .serve()
        })?;

        Ok(Cleanup::None)
    }
}

impl McpConnectionState {
    async fn start_connection(
        &self,
        _request: McpConnectRequest,
        request_cx: JsonRpcRequestCx<McpConnectResponse>,
    ) -> Result<(), acp::Error> {
        use crate::mcp_integration::mcp_server::TestMcpServer;

        // Generate connection ID and channel for future communication
        let connection_id = format!("mcp-{}", uuid::Uuid::new_v4());
        let (mcp_server_tx, mut mcp_server_rx) = mpsc::channel(128);
        self.connections
            .lock()
            .expect("not poisoned")
            .insert(connection_id.clone(), mcp_server_tx);

        // Generate streams
        let (mcp_server_stream, mcp_client_stream) = tokio::io::duplex(8192);
        let (mcp_server_read, mcp_server_write) = tokio::io::split(mcp_server_stream);
        let (mcp_client_read, mcp_client_write) = tokio::io::split(mcp_client_stream);

        // Create JsonRpcConnection for communicating the server
        request_cx.spawn(
            JsonRpcConnection::new(mcp_client_write.compat_write(), mcp_client_read.compat())
                // Everything the server sends us, we send to our agent
                .on_receive_request({
                    let connection_id = connection_id.clone();
                    let outer_cx = request_cx.json_rpc_cx();
                    async move |mcp_request: UntypedMessage, mcp_request_cx| {
                        outer_cx
                            .send_request_to_successor(McpOverAcpRequest {
                                connection_id: connection_id.clone(),
                                request: mcp_request,
                            })
                            .forward_to_request_cx(mcp_request_cx)
                    }
                })
                .on_receive_notification({
                    let connection_id = connection_id.clone();
                    let outer_cx = request_cx.json_rpc_cx();
                    async move |mcp_notification: UntypedMessage, _| {
                        outer_cx.send_notification_to_successor(McpOverAcpNotification {
                            connection_id: connection_id.clone(),
                            notification: mcp_notification,
                        })
                    }
                })
                .with_client({
                    async move |mcp_cx| {
                        while let Some(msg) = mcp_server_rx.next().await {
                            mcp_cx.send_proxied_message(msg)?;
                        }
                        Ok(())
                    }
                }),
        )?;

        // Spawn MCP server task
        request_cx.spawn(async move {
            let server = TestMcpServer::new();
            server
                .serve((mcp_server_read, mcp_server_write))
                .await
                .map(|_running_server| ())
                .map_err(acp::Error::into_internal_error)
        })?;

        request_cx.respond(McpConnectResponse {
            connection_id: connection_id.into(),
        })
    }

    async fn send_proxied_message(
        &self,
        connection_id: String,
        message: ProxiedMessage,
    ) -> Result<(), acp::Error> {
        let mut mcp_tx = self
            .connections
            .lock()
            .expect("not poisoned")
            .get(&connection_id)
            .ok_or_else(|| {
                scp::util::internal_error(format!("unknown connection: {:?}", connection_id))
            })?
            .clone();
        mcp_tx
            .send(message)
            .await
            .map_err(acp::Error::into_internal_error)
    }
}

pub fn create() -> Box<dyn ComponentProvider> {
    Box::new(ProxyComponentProvider)
}
