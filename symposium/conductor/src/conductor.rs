//! # Conductor: P/ACP Proxy Chain Orchestrator
//!
//! This module implements the Fiedler conductor, which orchestrates a chain of
//! proxy components that sit between an editor and an agent, transforming the
//! Agent-Client Protocol (ACP) stream bidirectionally.
//!
//! ## Architecture Overview
//!
//! The conductor builds and manages a chain of components:
//!
//! ```text
//! Editor <-ACP-> [Component 0] <-ACP-> [Component 1] <-ACP-> ... <-ACP-> Agent
//! ```
//!
//! Each component receives ACP messages, can transform them, and forwards them
//! to the next component in the chain. The conductor:
//!
//! 1. Spawns each component as a subprocess
//! 2. Establishes bidirectional JSON-RPC connections with each component
//! 3. Routes messages between editor, components, and agent
//! 4. Manages the `_meta.symposium.proxy` capability to signal chain position
//!
//! ## Recursive Chain Building
//!
//! The chain is built recursively through the `_proxy/successor/*` protocol:
//!
//! 1. Editor connects to Component 0 via the conductor
//! 2. When Component 0 wants to communicate with its successor, it sends
//!    requests/notifications with method prefix `_proxy/successor/`
//! 3. The conductor intercepts these messages, strips the prefix, and forwards
//!    to Component 1
//! 4. Component 1 does the same for Component 2, and so on
//! 5. The last component talks directly to the agent (no `_proxy/successor/` prefix)
//!
//! This allows each component to be written as if it's talking to a single successor,
//! without knowing about the full chain.
//!
//! ## Capability Management
//!
//! Components discover their position in the chain via the `_meta.symposium.proxy`
//! capability in `initialize` requests:
//!
//! - **First component** (from editor): Receives proxy capability if chain has >1 components
//! - **Middle components**: Receive proxy capability to indicate they have a successor
//! - **Last component**: Does NOT receive proxy capability (talks directly to agent)
//!
//! The conductor manages this by:
//! - Adding proxy capability when editor sends initialize to first component (if chain has >1 components)
//! - Adding proxy capability when component sends initialize to successor (if successor is not last)
//! - Removing proxy capability when component sends initialize to last component
//!
//! ## Message Routing
//!
//! The conductor runs an event loop processing messages from:
//!
//! - **Editor to first component**: Standard ACP messages
//! - **Component to successor**: Via `_proxy/successor/*` prefix
//! - **Component responses**: Via futures channels back to requesters
//!
//! The message flow ensures bidirectional communication while maintaining the
//! abstraction that each component only knows about its immediate successor.

use std::{collections::HashMap, pin::Pin};

use acp_proxy::{
    McpConnectRequest, McpConnectResponse, McpDisconnectNotification, McpOverAcpNotification,
    McpOverAcpRequest, SuccessorNotification, SuccessorRequest,
};
use agent_client_protocol::{
    self as acp, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};

use scp::{
    JsonRpcConnection, JsonRpcConnectionCx, JsonRpcNotification, JsonRpcRequest, JsonRpcRequestCx,
    JsonRpcResponse, MessageAndCx, MetaCapabilityExt, NullHandler, Proxy, TypeNotification,
    TypeRequest, UntypedMessage,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, info};

use crate::{
    component::{Component, ComponentProvider},
    conductor::mcp_bridge::{McpBridgeConnection, McpBridgeConnectionActor, McpBridgeListeners},
};

mod mcp_bridge;

/// Arguments for the serve method, containing I/O streams.
///
/// These are kept separate from the Conductor struct to avoid partial move issues.
struct ServeArgs<OB: AsyncWrite, IB: AsyncRead> {
    connection: JsonRpcConnection<OB, IB, NullHandler>,
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

/// The conductor manages the proxy chain lifecycle and message routing.
///
/// It maintains connections to all components in the chain and routes messages
/// bidirectionally between the editor, components, and agent.
///
pub struct Conductor {
    /// Channel for receiving internal conductor messages from spawned tasks
    conductor_rx: mpsc::Receiver<ConductorMessage>,

    /// Manages the TCP listeners for MCP connections that will be proxied over ACP.
    bridge_listeners: McpBridgeListeners,

    /// Manages active connections to MCP clients.
    bridge_connections: HashMap<String, McpBridgeConnection>,

    /// The chain of spawned components, ordered from first (index 0) to last
    components: Vec<Component>,

    /// Command and args to spawn conductor MCP bridge processes
    /// E.g., vec!["conductor"] or vec!["cargo", "run", "-p", "conductor", "--"]
    conductor_command: Vec<String>,
}

impl Conductor {
    pub async fn run<OB: AsyncWrite, IB: AsyncRead>(
        outgoing_bytes: OB,
        incoming_bytes: IB,
        providers: Vec<Box<dyn ComponentProvider>>,
    ) -> Result<(), acp::Error> {
        Self::run_with_command(outgoing_bytes, incoming_bytes, providers, None).await
    }

    pub async fn run_with_command<OB: AsyncWrite, IB: AsyncRead>(
        outgoing_bytes: OB,
        incoming_bytes: IB,
        mut providers: Vec<Box<dyn ComponentProvider>>,
        conductor_command: Option<Vec<String>>,
    ) -> Result<(), acp::Error> {
        tracing::debug!(
            incoming_conductor_command = ?conductor_command,
            "run_with_command called"
        );
        let conductor_command = conductor_command.unwrap_or_else(|| {
            vec![
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
                    .unwrap_or_else(|| "conductor".to_string()),
            ]
        });
        tracing::debug!(
            resolved_conductor_command = ?conductor_command,
            "conductor_command after unwrap_or_else"
        );
        if providers.len() == 0 {
            return Err(scp::util::internal_error(
                "must have at least one component",
            ));
        }

        info!(
            component_count = providers.len(),
            "Starting conductor with component chain"
        );

        providers.reverse();
        let (conductor_tx, conductor_rx) = mpsc::channel(128 /* chosen arbitrarily */);

        let connection =
            JsonRpcConnection::new(outgoing_bytes, incoming_bytes).name("client-to-conductor");

        let serve_args = ServeArgs {
            connection,
            conductor_tx: conductor_tx.clone(),
        };

        Conductor {
            components: Default::default(),
            bridge_listeners: Default::default(),
            bridge_connections: Default::default(),
            conductor_rx,
            conductor_command,
        }
        .launch_proxy(providers, serve_args)
        .await
    }

    /// Recursively spawns components and builds the proxy chain.
    ///
    /// This function implements the recursive chain building pattern:
    /// 1. Pop the next component from the `providers` list
    /// 2. Create the component (either spawn subprocess or use mock)
    /// 3. Set up JSON-RPC connection and message handlers
    /// 4. Recursively call itself to spawn the next component
    /// 5. When no components remain, start the message routing loop via `serve()`
    ///
    /// Each component is given a channel to send messages back to the conductor,
    /// enabling the bidirectional message routing.
    ///
    /// # Arguments
    ///
    /// - `providers`: Stack of component providers (reversed, so we pop from the end)
    /// - `serve_args`: I/O streams and conductor channel for the serve method
    fn launch_proxy<OB: AsyncWrite, IB: AsyncRead>(
        mut self,
        mut providers: Vec<Box<dyn ComponentProvider>>,
        serve_args: ServeArgs<OB, IB>,
    ) -> Pin<Box<impl Future<Output = Result<(), acp::Error>>>> {
        Box::pin(async move {
            let Some(next_provider) = providers.pop() else {
                info!("All components spawned, starting message routing");
                return self.serve(serve_args).await;
            };

            let component_index = self.components.len();
            let remaining = providers.len();

            info!(
                component_index,
                remaining_components = remaining,
                "Creating component"
            );

            let (component_stream, conductor_stream) = tokio::io::duplex(1024); // buffer size

            // Split each side into read/write halves
            let (component_read, component_write) = tokio::io::split(component_stream);
            let (conductor_read, conductor_write) = tokio::io::split(conductor_stream);

            let cx = serve_args.connection.json_rpc_cx();

            // Create the component streams based on the provider type
            let cleanup = next_provider.create(
                &cx,
                Box::pin(component_write.compat_write()),
                Box::pin(component_read.compat()),
            )?;

            debug!(
                component_index,
                "Component created, setting up JSON-RPC connection"
            );

            JsonRpcConnection::new(conductor_write.compat_write(), conductor_read.compat())
                .name(format!("conductor-to-component({})", component_index))
                // Intercept messages sent by a proxy component (acting as ACP client) to its successor agent.
                .on_receive_message({
                    let mut conductor_tx = serve_args.conductor_tx.clone();
                    async move |message_cx: MessageAndCx<
                        SuccessorRequest<UntypedMessage>,
                        SuccessorNotification<UntypedMessage>,
                    >| {
                        conductor_tx
                            .send(ConductorMessage::ClientToAgent {
                                target_component_index: component_index + 1,
                                message: message_cx
                                    .map(|r, cx| (r.request, cx), |n, cx| (n.notification, cx)),
                            })
                            .await
                            .map_err(scp::util::internal_error)
                    }
                })
                // The proxy sees the conductor as its "client",
                // so if it sends a normal ACP message
                // (i.e., an agent-to-client message),
                // the conductor will forward that
                // to the proxy's predecessor.
                .on_receive_message({
                    let mut conductor_tx = serve_args.conductor_tx.clone();
                    async move |message_cx: MessageAndCx<UntypedMessage, UntypedMessage>| {
                        conductor_tx
                            .send(ConductorMessage::AgentToClient {
                                source_component_index: component_index,
                                message: message_cx,
                            })
                            .await
                            .map_err(scp::util::internal_error)
                    }
                })
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component {
                        cleanup,
                        agent_cx: jsonrpccx,
                    });
                    self.launch_proxy(providers, serve_args)
                        .await
                        .map_err(scp::util::internal_error)
                })
                .await
        })
    }

    /// Runs the main message routing loop after all components are spawned.
    ///
    /// This function processes messages from three sources:
    ///
    /// 1. **Editor → Component 0**: Messages from the editor go to the first component
    /// 2. **Component → Successor**: Messages prefixed with `_proxy/successor/*` are
    ///    routed to the next component in the chain
    /// 3. **Component → Client**: Responses and notifications flow backward to the
    ///    component's client (either editor or predecessor component)
    ///
    /// The routing ensures:
    /// - Capability management for `initialize` requests based on chain position
    /// - Proper prefix stripping for `_proxy/successor/*` messages
    /// - Bidirectional communication between all parts of the chain
    ///
    /// # Arguments
    ///
    /// - `serve_args`: I/O streams and conductor channel
    async fn serve<OB: AsyncWrite, IB: AsyncRead>(
        mut self,
        serve_args: ServeArgs<OB, IB>,
    ) -> Result<(), acp::Error> {
        let ServeArgs {
            connection,
            mut conductor_tx,
        } = serve_args;

        connection
            // Any incoming messages from the client are client-to-agent messages targeting the first component.
            .on_receive_message({
                let mut conductor_tx = conductor_tx.clone();
                async move |message: MessageAndCx| {
                    conductor_tx
                        .send(ConductorMessage::ClientToAgent {
                            target_component_index: 0,
                            message,
                        })
                        .await
                        .map_err(scp::util::internal_error)
                }
            })
            .with_client({
                async |client| {
                    // This is the "central actor" of the conductor. Most other things forward messages
                    // via `conductor_tx` into this loop. This lets us serialize the conductor's activity.
                    while let Some(message) = self.conductor_rx.next().await {
                        self.handle_conductor_message(&client, message, &mut conductor_tx)
                            .await?;
                    }
                    Ok(())
                }
            })
            .await
    }

    /// Central message handling logic for the conductor.
    /// The conductor routes all [`ConductorMessage`] messages through to this function.
    /// Each message corresponds to a request or notification from one component to another.
    /// The conductor ferries messages from one place to another, sometimes making modifications along the way.
    /// Note that *responses to requests* are sent *directly* without going through this loop.
    ///
    /// The names we use are
    ///
    /// * The *client* is the originator of all ACP traffic, typically an editor or GUI.
    /// * Then there is a sequence of *components* consisting of:
    ///     * Zero or more *proxies*, which receive messages and forward them to the next component in the chain.
    ///     * And finally the *agent*, which is the final component in the chain and handles the actual work.
    ///
    /// For the most part, we just pass messages through the chain without modification, but there are a few exceptions:
    ///
    /// * We insert the "proxy" capability to initialization messages going to proxy components (and remove it for the agent component).
    /// * We modify "session/new" requests that use `acp:...` as the URL for an MCP server to redirect
    ///   through a stdio server that runs on localhost and bridges messages.
    async fn handle_conductor_message(
        &mut self,
        client: &JsonRpcConnectionCx,
        message: ConductorMessage,
        conductor_tx: &mut mpsc::Sender<ConductorMessage>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::debug!(?message, "handle_conductor_message");

        match message {
            ConductorMessage::ClientToAgent {
                target_component_index,
                message,
            } => {
                self.forward_client_to_agent_message(
                    conductor_tx,
                    target_component_index,
                    message,
                    client,
                )
                .await
            }

            ConductorMessage::AgentToClient {
                source_component_index,
                message,
            } => self.send_message_to_predecessor_of(client, source_component_index, message),

            // New MCP connection request. Send it back along the chain to get a connection id.
            // When the connection id arrives, send a message back into this conductor loop with
            // the connection id and the (as yet unspawned) actor.
            ConductorMessage::McpConnectionReceived {
                acp_url,
                connection,
                actor,
            } => self
                .send_request_to_predecessor_of(
                    client,
                    self.components.len() - 1,
                    McpConnectRequest { acp_url },
                )
                .await_when_result_received({
                    let mut conductor_tx = conductor_tx.clone();
                    async move |result| {
                        match result {
                            Ok(response) => conductor_tx
                                .send(ConductorMessage::McpConnectionEstablished {
                                    response,
                                    actor,
                                    connection,
                                })
                                .await
                                .map_err(|_| acp::Error::internal_error()),
                            Err(_) => {
                                // Error occurred, just drop the connection.
                                Ok(())
                            }
                        }
                    }
                }),

            // MCP connection successfully established. Spawn the actor
            // and insert the connection into our map fot future reference.
            ConductorMessage::McpConnectionEstablished {
                response: McpConnectResponse { connection_id },
                actor,
                connection,
            } => {
                self.bridge_connections
                    .insert(connection_id.clone(), connection);
                client.spawn(actor.run(connection_id))
            }

            // Message meant for the MCP client received. Forward it to the appropriate actor's mailbox.
            ConductorMessage::McpClientToMcpServer {
                connection_id,
                message,
            } => {
                let wrapped = message.map(
                    |request, request_cx| {
                        (
                            McpOverAcpRequest {
                                connection_id: connection_id.clone(),
                                request,
                            },
                            request_cx,
                        )
                    },
                    |notification, notification_cx| {
                        (
                            McpOverAcpNotification {
                                connection_id: connection_id.clone(),
                                notification,
                            },
                            notification_cx,
                        )
                    },
                );
                self.send_message_to_predecessor_of(client, self.components.len() - 1, wrapped)
            }

            // MCP client disconnected. Remove it from our map and send the
            // notification backwards along the chain.
            ConductorMessage::McpConnectionDisconnected { notification } => {
                self.bridge_connections.remove(&notification.connection_id);
                self.send_notification_to_predecessor_of(
                    client,
                    self.components.len() - 1,
                    notification,
                )
            }
        }
    }

    /// Send a message (request or notification) to the predecessor of the given component.
    ///
    /// This is a bit subtle because the relationship of the conductor
    /// is different depending on who will be receiving the message:
    /// * If the message is going to the conductor's client, then no changes
    ///   are needed, as the conductor is sending an agent-to-client message and
    ///   the conductor is acting as the agent.
    /// * If the message is going to a proxy component, then we have to wrap
    ///   it in a "from successor" wrapper, because the conductor is the
    ///   proxy's client.
    fn send_message_to_predecessor_of<Req: JsonRpcRequest, N: JsonRpcNotification>(
        &mut self,
        client: &JsonRpcConnectionCx,
        source_component_index: usize,
        message: MessageAndCx<Req, N>,
    ) -> Result<(), acp::Error>
    where
        Req::Response: Send,
    {
        if source_component_index == 0 {
            client.send_proxied_message(message)
        } else {
            let wrapped = message.map(
                |request, request_cx| (SuccessorRequest { request }, request_cx),
                |notification, notification_cx| {
                    (SuccessorNotification { notification }, notification_cx)
                },
            );
            self.components[source_component_index - 1]
                .agent_cx
                .send_proxied_message(wrapped)
        }
    }

    fn send_request_to_predecessor_of<Req: JsonRpcRequest>(
        &mut self,
        client: &JsonRpcConnectionCx,
        source_component_index: usize,
        request: Req,
    ) -> JsonRpcResponse<Req::Response> {
        if source_component_index == 0 {
            client.send_request(request)
        } else {
            self.components[source_component_index - 1]
                .agent_cx
                .send_request(SuccessorRequest { request })
        }
    }

    /// Send a notification to the predecessor of the given component.
    ///
    /// This is a bit subtle because the relationship of the conductor
    /// is different depending on who will be receiving the message:
    /// * If the notification is going to the conductor's client, then no changes
    ///   are needed, as the conductor is sending an agent-to-client message and
    ///   the conductor is acting as the agent.
    /// * If the notification is going to a proxy component, then we have to wrap
    ///   it in a "from successor" wrapper, because the conductor is the
    ///   proxy's client.
    fn send_notification_to_predecessor_of<N: JsonRpcNotification>(
        &mut self,
        client: &JsonRpcConnectionCx,
        component_index: usize,
        notification: N,
    ) -> Result<(), acp::Error> {
        if component_index == 0 {
            client.send_notification(notification)
        } else {
            self.components[component_index - 1]
                .agent_cx
                .send_notification(SuccessorNotification { notification })
        }
    }

    /// Send a message (request or notification) from 'left to right'.
    /// Left-to-right means from the client or an intermediate proxy to the component
    /// at `target_component_index` (could be a proxy or the agent).
    /// Makes changes to select messages along the way (e.g., `initialize` and `session/new`).
    async fn forward_client_to_agent_message(
        &mut self,
        conductor_tx: &mut mpsc::Sender<ConductorMessage>,
        target_component_index: usize,
        message: MessageAndCx,
        client: &JsonRpcConnectionCx,
    ) -> Result<(), agent_client_protocol::Error> {
        match message {
            MessageAndCx::Request(request, request_cx) => {
                self.forward_client_to_agent_request(
                    conductor_tx,
                    target_component_index,
                    request,
                    request_cx,
                )
                .await
            }
            MessageAndCx::Notification(notification, _cx) => {
                self.send_client_to_agent_notification(target_component_index, notification, client)
                    .await
            }
        }
    }

    /// Send a request from 'left to right', forwarding the reply
    /// to `request_cx`. Left-to-right means from the client or an
    /// intermediate proxy to the component at `target_component_index` (could be
    /// a proxy or the agent). Makes changes to select messages
    /// along the way (e.g., `initialize` and `session/new`).
    async fn forward_client_to_agent_request(
        &mut self,
        conductor_tx: &mut mpsc::Sender<ConductorMessage>,
        target_component_index: usize,
        request: UntypedMessage,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), agent_client_protocol::Error> {
        TypeRequest::new(request, request_cx)
            .handle_if(async |request: InitializeRequest, request_cx| {
                // When forwarding "initialize", we either add or remove the proxy capability,
                // depending on whether we are sending this message to the final component.
                self.forward_initialize_request(target_component_index, request, request_cx)
            })
            .await
            .handle_if(async |request: NewSessionRequest, request_cx| {
                // When forwarding "session/new", we adjust MCP servers to manage "acp:" URLs.
                self.forward_session_new_request(
                    target_component_index,
                    request,
                    &conductor_tx,
                    request_cx,
                )
                .await
            })
            .await
            .handle_if(
                async |request: McpOverAcpRequest<UntypedMessage>, request_cx| {
                    let McpOverAcpRequest {
                        connection_id,
                        request: mcp_request,
                    } = request;
                    self.bridge_connections
                        .get_mut(&connection_id)
                        .ok_or_else(|| {
                            scp::util::internal_error(format!(
                                "unknown connection id: {}",
                                connection_id
                            ))
                        })?
                        .send(MessageAndCx::Request(mcp_request, request_cx))
                        .await
                },
            )
            .await
            .otherwise(async |request: UntypedMessage, request_cx| {
                // Handle other types of requests here
                // Otherwise, just send the message along "as is".
                self.components[target_component_index]
                    .agent_cx
                    .send_request(request)
                    .forward_to_request_cx(request_cx)
            })
            .await
    }

    /// Send a notification from 'left to right'.
    /// Left-to-right means from the client or an intermediate proxy to the component
    /// at `target_component_index` (could be a proxy or the agent).
    async fn send_client_to_agent_notification(
        &mut self,
        target_component_index: usize,
        notification: UntypedMessage,
        cx: &JsonRpcConnectionCx,
    ) -> Result<(), agent_client_protocol::Error> {
        let cx_clone = cx.clone();
        TypeNotification::new(notification, cx)
            .handle_if(
                async |notification: McpOverAcpNotification<UntypedMessage>| {
                    let McpOverAcpNotification {
                        connection_id,
                        notification: mcp_notification,
                    } = notification;
                    self.bridge_connections
                        .get_mut(&connection_id)
                        .ok_or_else(|| {
                            scp::util::internal_error(format!(
                                "unknown connection id: {}",
                                connection_id
                            ))
                        })?
                        .send(MessageAndCx::Notification(mcp_notification, cx_clone))
                        .await
                },
            )
            .await
            .otherwise(async |notification| {
                // Otherwise, just send the message along "as is".
                self.components[target_component_index]
                    .agent_cx
                    .send_notification(notification)
            })
            .await
    }

    /// Checks if the given component index is the agent (final component).
    fn is_agent_component(&self, component_index: usize) -> bool {
        component_index == self.components.len() - 1
    }

    /// Checks if the given component index is the last proxy before the agent.
    fn forward_initialize_request(
        &self,
        target_component_index: usize,
        mut initialize_req: InitializeRequest,
        request_cx: JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        // The conductor does not accept proxy capabilities.
        if initialize_req.has_meta_capability(Proxy) {
            return Err(scp::util::internal_error(
                "conductor received unexpected initialization request with proxy capability",
            ));
        }

        // Either add or remove proxy, depending on whether this component has a successor.
        let is_agent = self.is_agent_component(target_component_index);
        if is_agent {
            self.components[target_component_index]
                .agent_cx
                .send_request(initialize_req)
                .await_when_result_received(async move |response| match response {
                    Ok(response) => request_cx.respond(response),
                    Err(error) => request_cx.respond_with_error(error),
                })
        } else {
            initialize_req = initialize_req.add_meta_capability(Proxy);
            self.components[target_component_index]
                .agent_cx
                .send_request(initialize_req)
                .await_when_result_received(async move |response| match response {
                    Ok(mut response) => {
                        // Verify proxy capability handshake for non-agent components
                        // Each proxy component must respond with Proxy capability or we
                        // abort the conductor.
                        if !response.has_meta_capability(Proxy) {
                            return Err(scp::util::internal_error(format!(
                                "component {} is not a proxy",
                                target_component_index
                            )));
                        }

                        // We don't want to respond with that proxy capability to the predecessor.
                        // Proxy communication is just between the conductor and others.
                        response = response.remove_meta_capability(Proxy);

                        request_cx.respond(response)
                    }
                    Err(error) => request_cx.respond_with_error(error),
                })
        }
    }

    // Intercept `session/new` requests and replace MCP servers based on `acp:...` URLs with stdio-based servers.
    async fn forward_session_new_request(
        &mut self,
        target_component_index: usize,
        mut request: acp::NewSessionRequest,
        conductor_tx: &mpsc::Sender<ConductorMessage>,
        request_cx: JsonRpcRequestCx<NewSessionResponse>,
    ) -> Result<(), acp::Error> {
        // Before forwarding the ACP request to the agent, replace ACP servers with stdio-based servers.
        if self.is_agent_component(target_component_index) {
            for mcp_server in &mut request.mcp_servers {
                self.bridge_listeners
                    .transform_mcp_servers(
                        &request_cx,
                        mcp_server,
                        conductor_tx,
                        &self.conductor_command,
                    )
                    .await?;
            }
        }

        self.components[target_component_index]
            .agent_cx
            .send_request(request)
            .forward_to_request_cx(request_cx)
    }
}

/// Messages sent to the conductor's main event loop for routing.
///
/// These messages enable the conductor to route communication between:
/// - The editor and the first component
/// - Components and their successors in the chain
/// - Components and their clients (editor or predecessor)
///
/// All spawned tasks send messages via this enum through a shared channel,
/// allowing centralized routing logic in the `serve()` loop.
#[derive(Debug)]
pub enum ConductorMessage {
    /// A message (request or notification) targeting a component from its client.
    /// This message will be forwarded "as is" to the component.
    ClientToAgent {
        target_component_index: usize,
        message: MessageAndCx,
    },

    /// A message (request or notification) sent by a component to its client.
    /// This message will be forwarded "as is" to its client.
    AgentToClient {
        source_component_index: usize,
        message: MessageAndCx,
    },

    /// A pending MCP bridge connection request request.
    /// The request must be sent back over ACP to receive the connection-id.
    /// Once the connection-id is received, the actor must be spawned.
    McpConnectionReceived {
        /// The acp:$UUID URL identifying this bridge
        acp_url: String,

        /// The actor that should be spawned once the connection-id is available.
        actor: McpBridgeConnectionActor,

        /// The connection to the bridge
        connection: McpBridgeConnection,
    },

    /// A pending MCP bridge connection request request.
    /// The request must be sent back over ACP to receive the connection-id.
    /// Once the connection-id is received, the actor must be spawned.
    McpConnectionEstablished {
        response: McpConnectResponse,

        /// The actor that should be spawned once the connection-id is available.
        actor: McpBridgeConnectionActor,

        /// The connection to the bridge
        connection: McpBridgeConnection,
    },

    /// MCP message (request or notification) received from a bridge that needs to be routed to the final proxy.
    ///
    /// Sent when the bridge receives an MCP tool call from the agent and forwards it
    /// to the conductor via TCP. The conductor routes this to the appropriate proxy component.
    McpClientToMcpServer {
        connection_id: String,
        message: MessageAndCx,
    },

    /// Message sent when MCP client disconnects
    McpConnectionDisconnected {
        notification: McpDisconnectNotification,
    },
}
