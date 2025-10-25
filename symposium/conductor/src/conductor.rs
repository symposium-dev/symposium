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

use agent_client_protocol::{self as acp, ClientRequest, InitializeRequest, NewSessionRequest};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};
use scp::{
    AcpAgentToClientMessages, AcpClientToAgentMessages, InitializeRequestExt,
    InitializeResponseExt, JsonRpcConnection, JsonRpcConnectionCx, JsonRpcOutgoingMessage,
    JsonRpcRequest, JsonRpcRequestCx, Proxy, ProxyToConductorMessages, UntypedMessage,
    UntypedNotification, util::json_cast,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{Instrument, debug, error, info, warn};

use crate::component::{Component, ComponentProvider};

/// Information about an MCP bridge for routing messages.
///
/// When a component provides an MCP server with ACP transport (`acp:$UUID`),
/// and the agent lacks native `mcp_acp_transport` support, the conductor
/// spawns a TCP listener and transforms the server spec to use stdio transport.
#[derive(Clone)]
struct McpBridgeInfo {
    /// The original acp:$UUID URL from the MCP server specification
    acp_url: String,
    /// The TCP port we bound for this bridge
    tcp_port: u16,
    /// The JSON-RPC connection to the bridge process (once connected)
    bridge_cx: Option<JsonRpcConnectionCx>,
}

/// Arguments for the serve method, containing I/O streams.
///
/// These are kept separate from the Conductor struct to avoid partial move issues.
struct ServeArgs<OB: AsyncWrite, IB: AsyncRead> {
    conductor_tx: mpsc::Sender<ConductorMessage>,
    outgoing_bytes: OB,
    incoming_bytes: IB,
}

/// The conductor manages the proxy chain lifecycle and message routing.
///
/// It maintains connections to all components in the chain and routes messages
/// bidirectionally between the editor, components, and agent.
///
pub struct Conductor {
    /// Channel for receiving internal conductor messages from spawned tasks
    conductor_rx: mpsc::Receiver<ConductorMessage>,
    /// The chain of spawned components, ordered from first (index 0) to last
    components: Vec<Component>,
    /// Whether the agent (last component) needs MCP bridging (lacks mcp_acp_transport capability)
    agent_needs_mcp_bridging: Option<bool>,
    /// Mapping of acp:$UUID URLs to TCP bridge information for MCP message routing
    mcp_bridges: HashMap<String, McpBridgeInfo>,
}

impl Conductor {
    pub async fn run<OB: AsyncWrite, IB: AsyncRead>(
        outgoing_bytes: OB,
        incoming_bytes: IB,
        mut providers: Vec<ComponentProvider>,
    ) -> anyhow::Result<()> {
        if providers.len() == 0 {
            anyhow::bail!("must have at least one component")
        }

        info!(
            component_count = providers.len(),
            "Starting conductor with component chain"
        );

        providers.reverse();
        let (conductor_tx, conductor_rx) = mpsc::channel(128 /* chosen arbitrarily */);

        tokio::task::LocalSet::new()
            .run_until(async move {
                let serve_args = ServeArgs {
                    conductor_tx: conductor_tx.clone(),
                    outgoing_bytes,
                    incoming_bytes,
                };

                Conductor {
                    components: Default::default(),
                    conductor_rx,
                    agent_needs_mcp_bridging: None,
                    mcp_bridges: HashMap::new(),
                }
                .launch_proxy(providers, serve_args)
                .await
            })
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
        mut providers: Vec<ComponentProvider>,
        serve_args: ServeArgs<OB, IB>,
    ) -> Pin<Box<impl Future<Output = anyhow::Result<()>>>> {
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

            // Create the component streams based on the provider type
            let (child, stdin, stdout) = match next_provider {
                ComponentProvider::Command(command) => {
                    debug!(component_index, command = %command, "Spawning command");

                    let mut child = tokio::process::Command::new(&command)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .spawn()?;

                    // Take ownership of the streams (can only do this once!)
                    let stdin = child.stdin.take().expect("Failed to open stdin");
                    let stdout = child.stdout.take().expect("Failed to open stdout");

                    // Convert tokio streams to futures streams using compat
                    (
                        Some(child),
                        Box::pin(stdin.compat_write()) as Pin<Box<dyn AsyncWrite + Send>>,
                        Box::pin(stdout.compat()) as Pin<Box<dyn AsyncRead + Send>>,
                    )
                }
                #[cfg(any(test, feature = "test-support"))]
                ComponentProvider::Mock(mock) => {
                    debug!(component_index, "Creating mock component");
                    // mock is Box<dyn MockComponent>, create() takes Box<Self>
                    let (outgoing, incoming) = mock.create().await?;
                    (None, outgoing, incoming)
                }
            };

            debug!(
                component_index,
                "Component created, setting up JSON-RPC connection"
            );

            JsonRpcConnection::new(stdin, stdout)
                // The proxy sees the conductor as its "client",
                // so if it sends a normal ACP message
                // (i.e., an agent-to-client message),
                // the conductor will forward that
                // to the proxy's predecessor.
                .on_receive(AcpAgentToClientMessages::send_to({
                    let mut conductor_tx = serve_args.conductor_tx.clone();
                    async move |message| {
                        conductor_tx
                            .send(ConductorMessage::ComponentToItsPredecessorMessage {
                                component_index,
                                message,
                            })
                            .await
                    }
                }))
                // When the proxy sends a `_proxy/successor/send/{request,notification}` message,
                // the conductor will forward that to the proxy's successor.
                .on_receive(ProxyToConductorMessages::callback(SuccessorSendCallbacks {
                    component_index,
                    conductor_tx: serve_args.conductor_tx.clone(),
                }))
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component {
                        child,
                        agent_cx: jsonrpccx,
                    });
                    self.launch_proxy(providers, serve_args)
                        .await
                        .map_err(scp::util::internal_error)
                })
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))
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
    ) -> anyhow::Result<()> {
        let conductor_tx = serve_args.conductor_tx;
        JsonRpcConnection::new(serve_args.outgoing_bytes, serve_args.incoming_bytes)
            .on_receive(AcpClientToAgentMessages::send_to({
                // When we receive messages from the client, forward to the first item
                // the proxy chain.
                let mut conductor_tx_clone = conductor_tx.clone();
                async move |message| {
                    conductor_tx_clone
                        .send(ConductorMessage::ClientToAgentViaProxyChain { message })
                        .await
                }
            }))
            .with_client(async |client| {
                // This is the "central actor" of the conductor. Most other things forward messages
                // via `conductor_tx` into this loop. This lets us serialize the conductor's activity.
                while let Some(message) = self.conductor_rx.next().await {
                    self.handle_conductor_message(&client, message, &conductor_tx)
                        .await?;
                }
                Ok(())
            })
            .await
            .map_err(|err| anyhow::anyhow!("{err:?}"))
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
        conductor_tx: &mpsc::Sender<ConductorMessage>,
    ) -> Result<(), agent_client_protocol::Error> {
        match message {
            // # Messages coming from the client:
            //
            // * these are sent to the first component in the chain (could be a proxy or the agent)
            ConductorMessage::ClientToAgentViaProxyChain { message } => match message {
                // Initialize requests: insert or remove proxy capability.
                scp::AcpClientToAgentMessage::Request(
                    ClientRequest::InitializeRequest(init_req),
                    json_rpc_request_cx,
                ) => {
                    self.send_initialize_request(0, Ok(init_req), json_rpc_request_cx)
                        .await?;
                }

                // Other requests: forward to the first component.
                scp::AcpClientToAgentMessage::Request(client_request, request_cx) => {
                    let method = client_request.method().to_string();

                    debug!(
                        method,
                        target = "component_0",
                        has_successor = self.components.len() > 1,
                        "Routing editor request to first component"
                    );

                    self.components[0]
                        .agent_cx
                        .send_request(client_request)
                        .forward_to_request_cx(request_cx)
                        .await?;
                }

                // Notifications: forward to the first component.
                scp::AcpClientToAgentMessage::Notification(client_notification, _json_rpc_cx) => {
                    debug!(
                        method = client_notification.method(),
                        target = "component_0",
                        "Routing editor notification to first component"
                    );
                    self.components[0]
                        .agent_cx
                        .send_notification(client_notification)?
                }
            },

            // # Message coming from a component to its *predecessor*.
            //
            // This is a bit subtle. The component is sending an agent-to-client
            // message to its predecessor. The way the conductor handles this depends
            // on the conductor's relationship to that predecessor:
            //
            // * If the predecessor is the client, then the conductor is its agent,
            //   and it can just send this as normal ACP.
            // * If the predecessor is another component, then the conductor is acting
            //   as the predecessor's *client*, and hence it has to send the message
            //   wrapped in a "from successor" wrapping.
            ConductorMessage::ComponentToItsPredecessorMessage {
                component_index,
                message,
            } => match message {
                scp::AcpAgentToClientMessage::Request(request, request_cx) => {
                    let method = request.method().to_string();

                    debug!(
                        component_index,
                        method, "Routing component request to its client"
                    );

                    if component_index == 0 {
                        client
                            .send_request(request)
                            .forward_to_request_cx(request_cx)?;
                    } else {
                        self.components[component_index - 1]
                            .agent_cx
                            .send_request(scp::FromSuccessorRequest {
                                method,
                                params: request,
                            })
                            .forward_to_request_cx(request_cx)?;
                    }
                }

                scp::AcpAgentToClientMessage::Notification(agent_notification, _) => {
                    debug!(
                        component_index,
                        method = agent_notification.method(),
                        "Routing component notification to its client"
                    );

                    if component_index == 0 {
                        client.send_notification(agent_notification)?;
                    } else {
                        self.components[component_index - 1]
                            .agent_cx
                            .send_notification(scp::FromSuccessorNotification {
                                method: agent_notification.method().to_string(),
                                params: agent_notification,
                            })?;
                    }
                }
            },

            // # Request coming from a component to its *successor*.
            //
            // The component is sending a client-to-agent request to its successor in the chain.
            // The conductor is always playing the client role for its components, so this is just a normal ACP message send.
            // However, we intercept some messages and make changes along the way.
            ConductorMessage::ComponentToItsSuccessorSendRequest {
                component_index,
                args: scp::ToSuccessorRequest { method, params },
                request_cx,
            } => {
                if method == "initialize" {
                    // When forwarding "initialize", we either add or remove the proxy capability,
                    // depending on whether we are sending this message to the final component.
                    self.send_initialize_request(
                        component_index,
                        serde_json::from_value(params).map_err(acp::Error::into_internal_error),
                        request_cx,
                    )
                    .await?;
                } else if method == "session/new" {
                    // When forwarding "session/new", we adjust MCP servers to manage "acp:" URLs.
                    self.send_session_new_request(
                        component_index,
                        serde_json::from_value(params).map_err(acp::Error::into_internal_error),
                        request_cx.cast(),
                        &conductor_tx,
                    )
                    .await?;
                } else {
                    // Otherwise, just send the message along "as is".
                    self.components[component_index + 1]
                        .agent_cx
                        .send_request(UntypedMessage::new(method, params))
                        .forward_to_request_cx(request_cx)?;
                }
            }

            // # Notification coming from a component to its *successor*.
            //
            // The component is sending a client-to-agent notification to its successor in the chain.
            // The conductor is always playing the client role for its components, so this is just a normal ACP message send.
            ConductorMessage::ComponentToItsSuccessorSendNotification {
                component_index,
                args: scp::ToSuccessorNotification { method, params },
                component_cx,
            } => {
                self.components[component_index + 1]
                    .agent_cx
                    .send_notification(UntypedNotification::new(method, params))?;
            }

            //
            ConductorMessage::McpRequestReceived {
                acp_url,
                method,
                params,
                response_cx,
            } => {
                info!(
                    acp_url = acp_url,
                    method = method,
                    "Bridge request received, routing to proxy via successor chain"
                );

                // Find which component owns this MCP server
                // For now, we'll route to the first component (component 0)
                // which should be the proxy that injected the MCP server
                // TODO: Track which component owns which MCP server UUID

                // Send the MCP request directly to the component via JSON-RPC
                // The component should handle MCP methods like tools/call, tools/list, etc.
                debug!(method = method, "Sending MCP request to component 0");

                let request = scp::UntypedMessage::new(method.clone(), params);
                let response = self.components[0].agent_cx.send_request(request);
                let method_for_task = method.clone();

                let _ = response.await_when_response_received(async move |result| {
                    async {
                        debug!(
                            is_ok = result.is_ok(),
                            "Received MCP response from component"
                        );
                        let _ = response_cx.respond_with_result(result);
                    }
                    .instrument(tracing::info_span!("bridge_request", method = %method_for_task))
                    .await
                });
            }

            ConductorMessage::BridgeConnected { acp_url, bridge_cx } => {
                info!(acp_url = acp_url, "Bridge connected, updating bridge info");

                // Update the bridge info with the connection
                if let Some(bridge_info) = self.mcp_bridges.get_mut(&acp_url) {
                    bridge_info.bridge_cx = Some(bridge_cx);
                    info!(
                        acp_url = acp_url,
                        tcp_port = bridge_info.tcp_port,
                        "Bridge connection stored for message routing"
                    );
                } else {
                    warn!(
                        acp_url = acp_url,
                        "Received bridge connection for unknown acp_url"
                    );
                }
            }

            ConductorMessage::Error { error } => {
                error!(
                    error_code = error.code,
                    error_message = %error.message,
                    "Error in spawned task"
                );
            }
        }
        Ok(())
    }

    /// Checks if the given component index is the agent (final component).
    fn is_agent_component(&self, component_index: usize) -> bool {
        component_index == self.components.len() - 1
    }

    /// Checks if the given component index is the last proxy before the agent.
    fn is_last_proxy_component(&self, component_index: usize) -> bool {
        self.components.len() > 1 && component_index == self.components.len() - 2
    }

    async fn send_initialize_request(
        &self,
        to_component: usize,
        initialize_req: Result<InitializeRequest, acp::Error>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), agent_client_protocol::Error> {
        // If we failed to create the initialize request, respond with an error and return.
        let Ok(mut initialize_req) = initialize_req else {
            request_cx.respond_with_error(agent_client_protocol::Error::parse_error())?;
            return Ok(());
        };

        // Either add or remove proxy, depending on whether this component has a successor.
        if self.is_agent_component(to_component) {
            initialize_req = initialize_req.remove_meta_capability(Proxy);
        } else {
            initialize_req = initialize_req.add_meta_capability(Proxy);
        }

        let response = self.components[to_component]
            .agent_cx
            .send_request(ClientRequest::InitializeRequest(initialize_req));

        response.await_when_response_received(async move |result| {
            request_cx.respond_with_result(result)
        })
    }

    /// Transforms MCP servers with `acp:$UUID` URLs for agents that need bridging.
    ///
    /// For each MCP server with an `acp:` URL:
    /// 1. Spawns a TCP listener on an ephemeral port
    /// 2. Stores the mapping for message routing
    /// 3. Transforms the server to use stdio transport pointing to `conductor mcp $PORT`
    ///
    /// Returns the modified NewSessionRequest with transformed MCP servers.
    async fn transform_mcp_servers(
        &mut self,
        mut request: NewSessionRequest,
        conductor_tx: &mpsc::Sender<ConductorMessage>,
    ) -> NewSessionRequest {
        use agent_client_protocol::McpServer;

        let mut transformed_servers = Vec::new();

        for server in request.mcp_servers {
            match server {
                McpServer::Http { name, url, headers } if url.starts_with("acp:") => {
                    info!(
                        server_name = name,
                        acp_url = url,
                        "Detected MCP server with ACP transport, spawning TCP bridge"
                    );

                    // Spawn TCP listener on ephemeral port
                    match self
                        .spawn_tcp_listener(url.clone(), conductor_tx.clone())
                        .await
                    {
                        Ok(tcp_port) => {
                            info!(
                                server_name = name,
                                acp_url = url,
                                tcp_port,
                                "Spawned TCP listener for MCP bridge"
                            );

                            // Transform to stdio transport pointing to conductor mcp process
                            let transformed = McpServer::Stdio {
                                name,
                                command: std::path::PathBuf::from("conductor"),
                                args: vec!["mcp".to_string(), tcp_port.to_string()],
                                env: vec![],
                            };
                            transformed_servers.push(transformed);
                        }
                        Err(e) => {
                            warn!(
                                server_name = name,
                                acp_url = url,
                                error = ?e,
                                "Failed to spawn TCP listener, keeping original server"
                            );
                            // Keep original server on error
                            transformed_servers.push(McpServer::Http { name, url, headers });
                        }
                    }
                }
                // Pass through other server types unchanged
                other_server => {
                    transformed_servers.push(other_server);
                }
            }
        }

        request.mcp_servers = transformed_servers;
        request
    }

    /// Routes an MCP request from agent to the appropriate bridge.
    ///
    /// Extracts the UUID from `_mcp/$UUID/$method` pattern, looks up the bridge,
    /// strips the `_mcp/$UUID/` prefix, and forwards to the bridge.
    ///
    /// Spawns a task to await the bridge's response and forward it back to the agent.
    ///
    /// Returns Some(()) if routing succeeded, None if bridge not found/connected.
    async fn route_to_mcp_bridge_request(
        &self,
        method: &str,
        request: impl serde::Serialize,
        response_cx: JsonRpcRequestCx<serde_json::Value>,
        mut conductor_tx: mpsc::Sender<ConductorMessage>,
    ) -> Option<()> {
        // Parse _mcp/$UUID/$actual_method
        let parts: Vec<&str> = method.splitn(4, '/').collect();
        if parts.len() < 3 || parts[0] != "" || parts[1] != "_mcp" {
            warn!(
                method = method,
                "Invalid _mcp/ method format, expected _mcp/$UUID/$method"
            );
            return None;
        }

        let uuid = parts[2];
        let actual_method = parts.get(3).copied().unwrap_or("");
        let acp_url = format!("acp:{}", uuid);

        // Look up bridge
        let bridge_info = self.mcp_bridges.get(&acp_url)?;
        let bridge_cx = bridge_info.bridge_cx.as_ref()?;

        info!(
            acp_url = acp_url,
            actual_method = actual_method,
            "Routing MCP request to bridge"
        );

        // Forward request with stripped method
        let params = serde_json::to_value(&request).ok().and_then(|v| match v {
            serde_json::Value::Object(map) => Some(jsonrpcmsg::Params::Object(map)),
            serde_json::Value::Array(arr) => Some(jsonrpcmsg::Params::Array(arr)),
            _ => None,
        });
        let request = scp::UntypedMessage::new(actual_method.to_string(), params);
        let response = bridge_cx.send_request(request);

        // Spawn task to await response and forward back to agent
        let request_id = response_cx.id().clone();
        let method_string = method.to_string();
        let current_span = tracing::Span::current();

        let _ = response.await_when_response_received(async move |result| {
            async {
                let is_ok = result.is_ok();
                debug!(
                    method = method_string,
                    is_ok, "Received bridge response, forwarding to agent"
                );

                if let Err(error) = conductor_tx
                    .send(ConductorMessage::ResponseReceived {
                        result,
                        response_cx,
                        method: method_string.clone(),
                        target_component_index: 0, // Response is from bridge to component 0
                    })
                    .await
                {
                    error!(
                        method = method_string,
                        ?error,
                        "Failed to send bridge response to conductor"
                    );
                } else {
                    debug!(
                        method = method_string,
                        "Sent bridge response to conductor for forwarding"
                    );
                }
            }
            .instrument(
                tracing::info_span!("receive_mcp_bridge_response", request_id = ?request_id),
            )
            .instrument(current_span)
            .await
        });

        Some(())
    }

    /// Spawns a TCP listener for an MCP bridge and stores the mapping.
    ///
    /// Binds to `localhost:0` to get an ephemeral port, then stores the
    /// `acp_url → tcp_port` mapping in `self.mcp_bridges`.
    ///
    /// Returns the bound port number.
    async fn spawn_tcp_listener(
        &mut self,
        acp_url: String,
        mut conductor_tx: mpsc::Sender<ConductorMessage>,
    ) -> anyhow::Result<u16> {
        use tokio::net::TcpListener;

        // Bind to ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let tcp_port = listener.local_addr()?.port();

        info!(
            acp_url = acp_url,
            tcp_port, "Bound TCP listener for MCP bridge"
        );

        // Store mapping for message routing (Phase 2b/3)
        self.mcp_bridges.insert(
            acp_url.clone(),
            McpBridgeInfo {
                acp_url: acp_url.clone(),
                tcp_port,
                bridge_cx: None, // Will be set when bridge connects
            },
        );

        // Phase 2b: Accept connections from `conductor mcp $PORT`
        let acp_url_for_task = acp_url.clone();

        tokio::task::spawn_local(async move {
            info!(
                acp_url = acp_url_for_task,
                tcp_port, "Waiting for bridge connection"
            );

            // Accept a single connection (bridge processes connect once)
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!(
                        acp_url = acp_url_for_task,
                        bridge_addr = ?addr,
                        "Bridge connected"
                    );

                    let (read_half, write_half) = stream.into_split();

                    // Establish bidirectional JSON-RPC connection
                    // The bridge will send MCP requests (tools/call, etc.) to the conductor
                    // The conductor can also send responses back
                    let connection =
                        JsonRpcConnection::new(write_half.compat_write(), read_half.compat());

                    // Handle incoming requests from the bridge AND keep the connection alive
                    let _ = connection
                        .on_receive(scp::GenericHandler::send_to({
                            let conductor_tx = conductor_tx.clone();
                            let acp_url_inner = acp_url_for_task.clone();
                            move |method, params, response_cx| {
                                let mut conductor_tx = conductor_tx.clone();
                                let acp_url = acp_url_inner.clone();
                                async move {
                                    info!(
                                        method = method,
                                        acp_url = acp_url,
                                        "Received request from bridge, forwarding to proxy"
                                    );

                                    // Forward the MCP request to the proxy via conductor
                                    let _ = conductor_tx
                                        .send(ConductorMessage::McpRequestReceived {
                                            acp_url,
                                            method,
                                            params,
                                            response_cx,
                                        })
                                        .await;

                                    Ok::<(), std::convert::Infallible>(())
                                }
                            }
                        }))
                        .with_client(async move |bridge_cx| {
                            // Notify conductor that bridge is connected
                            // This allows the conductor to send requests TO the bridge if needed
                            let _ = conductor_tx
                                .send(ConductorMessage::BridgeConnected {
                                    acp_url: acp_url_for_task.clone(),
                                    bridge_cx: bridge_cx.clone(),
                                })
                                .await;

                            // Keep connection alive until bridge disconnects
                            futures::future::pending::<()>().await;

                            Ok::<(), agent_client_protocol::Error>(())
                        })
                        .await;

                    Ok::<(), agent_client_protocol::Error>(())
                }
                Err(e) => {
                    warn!(
                        acp_url = acp_url_for_task,
                        error = ?e,
                        "Failed to accept bridge connection"
                    );
                    Ok(())
                }
            }
        });

        Ok(tcp_port)
    }

    // Intercept `session/new` requests and replace MCP servers based on `acp:...` URLs with stdio-based servers.
    async fn send_session_new_request(
        &mut self,
        component_index: usize,
        request: Result<acp::NewSessionRequest, agent_client_protocol::Error>,
        request_cx: JsonRpcRequestCx<acp::NewSessionResponse>,
        conductor_tx: &mpsc::Sender<ConductorMessage>,
    ) -> Result<(), acp::Error> {
        let Ok(mut new_session_req) = request else {
            request_cx.respond_with_error(agent_client_protocol::Error::invalid_params())?;
            return Ok(());
        };

        new_session_req = self
            .transform_mcp_servers(new_session_req, conductor_tx)
            .await;

        self.components[component_index + 1]
            .agent_cx
            .send_request(new_session_req)
            .forward_to_request_cx(request_cx)
    }
}

fn ignore_send_err<T>(_: Result<T, mpsc::SendError>) {}

struct SuccessorSendCallbacks {
    component_index: usize,
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

impl scp::ConductorCallbacks for SuccessorSendCallbacks {
    async fn successor_send_request(
        &mut self,
        args: scp::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx
            .send(ConductorMessage::ComponentToItsSuccessorSendRequest {
                component_index: self.component_index,
                args,
                request_cx: response,
            })
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)
    }

    async fn successor_send_notification(
        &mut self,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        cx: &scp::JsonRpcConnectionCx,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx
            .send(ConductorMessage::ComponentToItsSuccessorSendNotification {
                component_index: self.component_index,
                args,
                component_cx: cx.clone(),
            })
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)
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
pub enum ConductorMessage {
    /// Message from the editor to be routed through the proxy chain.
    ///
    /// Always sent to component 0, which then uses `_proxy/successor/*`
    /// to forward to subsequent components if needed.
    ClientToAgentViaProxyChain {
        message: scp::AcpClientToAgentMessage,
    },

    /// Message from a component back to its client.
    ///
    /// The client is either:
    /// - The editor (if `component_index == 0`)
    /// - The predecessor component (if `component_index > 0`)
    ///
    /// This handles responses and notifications flowing backward through the chain.
    ComponentToItsPredecessorMessage {
        component_index: usize,
        message: scp::AcpAgentToClientMessage,
    },

    /// Request from a component to its successor via `_proxy/successor/request`.
    ///
    /// The conductor strips the `_proxy/successor/` prefix and routes to
    /// `components[component_index + 1]`, managing capability modifications
    /// for `initialize` requests based on chain position.
    ComponentToItsSuccessorSendRequest {
        component_index: usize,
        args: scp::ToSuccessorRequest<serde_json::Value>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    },

    /// Notification from a component to its successor via `_proxy/successor/notification`.
    ///
    /// Similar to requests, but no response is expected. The conductor strips
    /// the prefix and forwards to the next component.
    ComponentToItsSuccessorSendNotification {
        component_index: usize,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        component_cx: JsonRpcConnectionCx,
    },

    /// Error from a spawned task that couldn't be handled locally.
    ///
    /// Currently logged as a warning. Future versions may trigger chain shutdown.
    Error { error: agent_client_protocol::Error },

    /// MCP request received from a bridge that needs to be routed to the proxy.
    ///
    /// Sent when the bridge receives an MCP tool call from the agent and forwards it
    /// to the conductor via TCP. The conductor routes this to the appropriate proxy component.
    McpRequestReceived {
        /// The acp:$UUID URL identifying which MCP server this request is for
        acp_url: String,
        /// The MCP method being called (e.g., "tools/call", "tools/list")
        method: String,
        /// The parameters for the MCP request
        params: Option<jsonrpcmsg::Params>,
        /// Context to send the response back to the bridge
        response_cx: JsonRpcRequestCx<serde_json::Value>,
    },

    /// MCP bridge connected and ready for message routing.
    ///
    /// Sent when a bridge process connects to the TCP listener. The conductor
    /// stores the bridge's JsonRpcCx to enable routing of `_mcp/$UUID/*` messages.
    BridgeConnected {
        /// The acp:$UUID URL identifying this bridge
        acp_url: String,
        /// The JSON-RPC connection to the bridge
        bridge_cx: JsonRpcConnectionCx,
    },
}
