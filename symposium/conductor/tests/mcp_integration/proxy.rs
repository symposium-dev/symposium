//! Proxy component that provides MCP tools

use agent_client_protocol::{
    self as acp, InitializeRequest, McpServer, NewSessionRequest, NewSessionResponse,
};
use conductor::component::{Cleanup, ComponentProvider};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};
use rmcp::ServiceExt;
use scp::{
    JsonRpcConnection, JsonRpcConnectionCx, JsonRpcCxExt, McpConnectRequest, McpConnectResponse,
    McpOverAcpNotification, McpOverAcpRequest, MetaCapabilityExt, Proxy, UntypedMessage,
};
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

pub struct ProxyComponentProvider;

/// Shared state for MCP connections
#[derive(Clone)]
struct McpConnectionState {
    /// Map of connection_id to channel for sending messages to MCP server
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<McpClientMessage>>>>,
}

/// Messages from MCP client to MCP server
enum McpClientMessage {
    Request {
        request: UntypedMessage,
        response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, acp::Error>>,
    },
    Notification {
        notification: UntypedMessage,
    },
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
            async move {
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
                    .on_receive_request({
                        let state = state.clone();
                        async move |request: McpConnectRequest, request_cx| {
                            // Spawn MCP server for this connection
                            let connection_id = format!("mcp-{}", uuid::Uuid::new_v4());

                            // Create channel for communicating with MCP server
                            let (client_tx, mut client_rx) = mpsc::channel::<McpClientMessage>(128);

                            // Store the connection
                            state
                                .connections
                                .lock()
                                .await
                                .insert(connection_id.clone(), client_tx);

                            // Spawn MCP server task
                            request_cx.spawn(async move {
                                use crate::mcp_integration::mcp_server::TestMcpServer;
                                use rmcp::{ClientHandler, ServiceExt};

                                let server = TestMcpServer::new();

                                // Create duplex stream for MCP communication
                                let (client_stream, server_stream) = tokio::io::duplex(8192);

                                // Spawn task to serve MCP server
                                tokio::spawn(async move {
                                    if let Err(e) = server.serve(server_stream).await {
                                        eprintln!("MCP server error: {:?}", e);
                                    }
                                });

                                // Create MCP client to communicate with the server
                                struct NoOpClientHandler;
                                impl ClientHandler for NoOpClientHandler {}

                                let mcp_client =
                                    NoOpClientHandler.serve(client_stream).await.map_err(|e| {
                                        eprintln!("MCP client serve error: {:?}", e);
                                        acp::Error::internal_error()
                                    })?;

                                // Handle messages from ACP client
                                while let Some(message) = client_rx.next().await {
                                    match message {
                                        McpClientMessage::Request {
                                            request,
                                            response_tx,
                                        } => {
                                            // Parse the method and forward to appropriate MCP call
                                            let result = if request.method == "tools/call" {
                                                // Call the tool via the MCP client
                                                match serde_json::from_value::<
                                                    rmcp::model::CallToolRequestParam,
                                                >(
                                                    request.params
                                                ) {
                                                    Ok(params) => {
                                                        match mcp_client
                                                            .peer()
                                                            .call_tool(params)
                                                            .await
                                                        {
                                                            Ok(result) => {
                                                                Ok(serde_json::to_value(&result)
                                                                    .unwrap())
                                                            }
                                                            Err(e) => {
                                                                eprintln!(
                                                                    "Tool call error: {:?}",
                                                                    e
                                                                );
                                                                Err(acp::Error::internal_error())
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        eprintln!(
                                                            "Failed to parse tool params: {:?}",
                                                            e
                                                        );
                                                        Err(acp::Error::invalid_params())
                                                    }
                                                }
                                            } else {
                                                eprintln!("Unsupported method: {}", request.method);
                                                Err(acp::Error::method_not_found())
                                            };

                                            let _ = response_tx.send(result);
                                        }
                                        McpClientMessage::Notification { notification: _ } => {
                                            // Notifications not yet implemented
                                        }
                                    }
                                }

                                // Clean up client
                                let _ = mcp_client.cancel().await;

                                Ok::<_, acp::Error>(())
                            })?;

                            request_cx.respond(McpConnectResponse {
                                connection_id: connection_id.into(),
                            })
                        }
                    })
                    .on_receive_request({
                        let state = state.clone();
                        async move |request: McpOverAcpRequest<UntypedMessage>, request_cx| {
                            let connection = state
                                .connections
                                .lock()
                                .await
                                .get(&request.connection_id)
                                .cloned();

                            if let Some(mut connection) = connection {
                                let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                                connection
                                    .send(McpClientMessage::Request {
                                        request: request.request,
                                        response_tx,
                                    })
                                    .await
                                    .map_err(|_| acp::Error::internal_error())?;

                                let result = response_rx
                                    .await
                                    .map_err(|_| acp::Error::internal_error())??;

                                request_cx.respond(result)
                            } else {
                                request_cx.respond_with_error(acp::Error::internal_error())
                            }
                        }
                    })
                    .on_receive_notification({
                        let state = state.clone();
                        async move |notification: McpOverAcpNotification<UntypedMessage>, _| {
                            let connection = state
                                .connections
                                .lock()
                                .await
                                .get(&notification.connection_id)
                                .cloned();

                            if let Some(mut connection) = connection {
                                let _ = connection
                                    .send(McpClientMessage::Notification {
                                        notification: notification.notification,
                                    })
                                    .await;
                            }

                            Ok(())
                        }
                    })
                    .serve()
                    .await
            }
        })?;

        Ok(Cleanup::None)
    }
}

pub fn create() -> Box<dyn ComponentProvider> {
    Box::new(ProxyComponentProvider)
}
