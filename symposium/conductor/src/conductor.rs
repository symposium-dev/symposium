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

use agent_client_protocol::{ClientRequest, InitializeRequest, NewSessionRequest};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};
use scp::{
    AcpAgentToClientMessages, AcpClientToAgentMessages, InitializeRequestExt,
    InitializeResponseExt, JsonRpcConnection, JsonRpcCx, JsonRpcNotification, JsonRpcRequest,
    JsonRpcRequestCx, Proxy, ProxyToConductorMessages,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{Instrument, debug, error, info, warn};

use crate::component::{Component, ComponentProvider};

/// Information about an MCP bridge for routing messages.
///
/// When a component provides an MCP server with ACP transport (`acp:$UUID`),
/// and the agent lacks native `mcp_acp_transport` support, the conductor
/// spawns a TCP listener and transforms the server spec to use stdio transport.
#[derive(Debug)]
struct McpBridgeInfo {
    /// The original acp:$UUID URL from the MCP server specification
    acp_url: String,
    /// The TCP port we bound for this bridge
    tcp_port: u16,
}

/// Arguments for the serve method, containing I/O streams.
///
/// These are kept separate from the Conductor struct to avoid partial move issues.
struct ServeArgs<OB: AsyncWrite, IB: AsyncRead> {
    conductor_tx: mpsc::Sender<ConductorMessage>,
    outgoing_bytes: OB,
    incoming_bytes: IB,
}

/// Manages the P/ACP proxy capability based on component position in the chain.
///
/// The proxy capability (`_meta.symposium.proxy`) signals to a component whether
/// it has a successor in the proxy chain:
/// - **Has successor**: Add the proxy capability
/// - **No successor (last component)**: Remove/omit the proxy capability
///
/// # Arguments
///
/// - `request`: The InitializeRequest to modify
/// - `component_index`: The index of the component receiving this request (0-based)
/// - `total_components`: Total number of components in the chain
///
/// # Returns
///
/// The modified InitializeRequest with capability added or removed as appropriate
fn manage_proxy_capability(
    request: InitializeRequest,
    component_index: usize,
    total_components: usize,
) -> InitializeRequest {
    let is_last_component = component_index == total_components - 1;

    if is_last_component {
        request.remove_meta_capability(Proxy)
    } else {
        request.add_meta_capability(Proxy)
    }
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
                // The proxy can send *editor* messages to use
                .on_receive(AcpAgentToClientMessages::send_to({
                    let mut conductor_tx = serve_args.conductor_tx.clone();
                    async move |message| {
                        conductor_tx
                            .send(ConductorMessage::ComponentToItsClientMessage {
                                component_index,
                                message,
                            })
                            .await
                    }
                }))
                .on_receive(ProxyToConductorMessages::callback(SuccessorSendCallbacks {
                    component_index,
                    conductor_tx: serve_args.conductor_tx.clone(),
                }))
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component { child, jsonrpccx });
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
                let mut conductor_tx_clone = conductor_tx.clone();
                async move |message| {
                    conductor_tx_clone
                        .send(ConductorMessage::ClientToAgentViaProxyChain { message })
                        .await
                }
            }))
            .with_client(async |client| {
                while let Some(message) = self.conductor_rx.next().await {
                    match message {
                        // When we receive messages from the client, forward to the first item
                        // the proxy chain.
                        ConductorMessage::ClientToAgentViaProxyChain { message } => match message {
                            // Special handling for initialize: manage proxy capability based on chain position
                            scp::AcpClientToAgentMessage::Request(
                                ClientRequest::InitializeRequest(init_req),
                                json_rpc_request_cx,
                            ) => {
                                let total_components = self.components.len();
                                let has_successor = total_components > 1;
                                let is_last_component = total_components == 1;
                                let method = "initialize";

                                debug!(
                                    method,
                                    target = "component_0",
                                    has_successor,
                                    total_components,
                                    "Routing initialization request to first component"
                                );

                                let modified_req = manage_proxy_capability(init_req, 0, total_components);
                                info!(
                                    target = "component_0",
                                    has_successor,
                                    "Managed proxy capability for first component"
                                );

                                send_request_and_forward_response(
                                    &self.components[0].jsonrpccx,
                                    ClientRequest::InitializeRequest(modified_req),
                                    json_rpc_request_cx.cast(),
                                    conductor_tx.clone(),
                                    method.to_string(),
                                    is_last_component,
                                )
                                .await;
                            }

                            scp::AcpClientToAgentMessage::Request(
                                mut client_request,
                                json_rpc_request_cx,
                            ) => {
                                let method = client_request.method().to_string();

                                // Special handling for NewSessionRequest: transform MCP servers if needed
                                if method == "newSession" && self.agent_needs_mcp_bridging == Some(true) {
                                    if let ClientRequest::NewSessionRequest(new_session_req) =
                                        client_request
                                    {
                                        info!("Intercepted new session request from editor, transforming MCP servers");
                                        let transformed = self.transform_mcp_servers(new_session_req).await;
                                        client_request = ClientRequest::NewSessionRequest(transformed);
                                    }
                                }

                                debug!(
                                    method,
                                    target = "component_0",
                                    has_successor = self.components.len() > 1,
                                    "Routing editor request to first component"
                                );


                                send_request_and_forward_response(
                                    &self.components[0].jsonrpccx,
                                    client_request,
                                    json_rpc_request_cx,
                                    conductor_tx.clone(),
                                    method,
                                    false, // Not from last component
                                )
                                .await;
                            }

                            scp::AcpClientToAgentMessage::Notification(
                                client_notification,
                                _json_rpc_cx,
                            ) => {
                                debug!(
                                    method = client_notification.method(),
                                    target = "component_0",
                                    "Routing editor notification to first component"
                                );
                                self.components[0]
                                    .jsonrpccx
                                    .send_notification(client_notification)?
                            }
                        },

                        ConductorMessage::ComponentToItsClientMessage {
                            component_index,
                            message,
                        } => {
                            let its_client: &JsonRpcCx = if component_index == 0 {
                                &client
                            } else {
                                &self.components[component_index - 1].jsonrpccx
                            };

                            let target = if component_index == 0 {
                                "editor"
                            } else {
                                "predecessor_component"
                            };

                            match message {
                                scp::AcpAgentToClientMessage::Request(
                                    agent_request,
                                    json_rpc_request_cx,
                                ) => {
                                    let method = agent_request.method().to_string();
                                    debug!(
                                        component_index,
                                        method,
                                        target,
                                        "Routing component request to its client"
                                    );
                                    send_request_and_forward_response(
                                        its_client,
                                        agent_request,
                                        json_rpc_request_cx,
                                        conductor_tx.clone(),
                                        method,
                                        false, // Not from last component
                                    )
                                    .await;
                                }
                                scp::AcpAgentToClientMessage::Notification(
                                    agent_notification,
                                    _json_rpc_cx,
                                ) => {
                                    debug!(
                                        component_index,
                                        method = agent_notification.method(),
                                        target,
                                        "Routing component notification to its client"
                                    );

                                    // If sending to a predecessor component (not the editor), wrap in FromSuccessorNotification
                                    if component_index > 0 {
                                        // Wrap the notification in the proxy format expected by on_receive_from_successor
                                        let params = serde_json::to_value(&agent_notification)
                                            .ok()
                                            .map(|v| match v {
                                                serde_json::Value::Object(map) => jsonrpcmsg::Params::Object(map),
                                                serde_json::Value::Array(arr) => jsonrpcmsg::Params::Array(arr),
                                                other => jsonrpcmsg::Params::Object(
                                                    vec![("value".to_string(), other)].into_iter().collect()
                                                ),
                                            });

                                        let wrapped = scp::FromSuccessorNotification {
                                            message: jsonrpcmsg::Request::notification_v2(
                                                agent_notification.method().to_string(),
                                                params,
                                            ),
                                        };
                                        its_client.send_notification(wrapped)?;
                                    } else {
                                        // Send directly to editor
                                        its_client.send_notification(agent_notification)?;
                                    }
                                }
                            }
                        }

                        ConductorMessage::ComponentToItsSuccessorSendRequest {
                            component_index,
                            args: scp::ToSuccessorRequest { method, params },
                            component_response_cx,
                        } => {
                            let successor_index = component_index + 1;

                            // Do transformations that require &mut self BEFORE borrowing from self.components
                            let (final_method, final_params) = if method == "initialize" {
                                // Try to parse params as InitializeRequest
                                if let Ok(init_req) = serde_json::from_value::<InitializeRequest>(params.clone()) {
                                    let total_components = self.components.len();
                                    let is_last_component = successor_index == total_components - 1;
                                    let modified_req = manage_proxy_capability(
                                        init_req,
                                        successor_index,
                                        total_components
                                    );

                                    info!(
                                        successor_index,
                                        is_last_component,
                                        total_components,
                                        "Managed proxy capability for successor component"
                                    );

                                    // Serialize back to params
                                    let modified_params = serde_json::to_value(modified_req)
                                        .unwrap_or(params.clone());
                                    (method, modified_params)
                                } else {
                                    (method, params)
                                }
                            } else if method == "newSession" && self.agent_needs_mcp_bridging == Some(true) {
                                // Try to parse params as NewSessionRequest
                                if let Ok(new_session_req) = serde_json::from_value::<NewSessionRequest>(params.clone()) {
                                    info!(
                                        component_index,
                                        successor_index,
                                        "Intercepted new session request from component, transforming MCP servers"
                                    );
                                    let transformed = self.transform_mcp_servers(new_session_req).await;
                                    let modified_params = serde_json::to_value(transformed)
                                        .unwrap_or(params.clone());
                                    (method, modified_params)
                                } else {
                                    (method, params)
                                }
                            } else {
                                (method, params)
                            };

                            // Now we can safely borrow from self.components
                            if let Some(successor_component) = self.components.get(successor_index)
                            {
                                let is_last_component = successor_index == self.components.len() - 1;

                                debug!(
                                    component_index,
                                    successor_index,
                                    method = %final_method,
                                    is_last_component,
                                    "Routing _proxy/successor/request to successor component"
                                );

                                let successor_response = successor_component
                                    .jsonrpccx
                                    .send_json_request(final_method.clone(), final_params);

                                let component_request_id = component_response_cx.id().clone();
                                let mut conductor_tx_clone = conductor_tx.clone();
                                let current_span = tracing::Span::current();
                                let method_clone = final_method.to_string();
                                tokio::task::spawn_local(
                                    async move {
                                        debug!("Waiting for successor response");
                                        let result = successor_response.recv().await;
                                        let is_ok = result.is_ok();
                                        debug!(is_ok, "Received successor response, sending to conductor");

                                        if let Err(error) = conductor_tx_clone
                                            .send(ConductorMessage::SuccessorResponseReceived {
                                                result,
                                                component_response_cx,
                                                method: method_clone,
                                                from_last_component: is_last_component,
                                            })
                                            .await
                                        {
                                            error!(?error, "Failed to send successor response to conductor");
                                        } else {
                                            debug!("Sent successor response to conductor for forwarding");
                                        }
                                    }
                                    .instrument(tracing::info_span!(
                                        "receive_successor_response",
                                        component_request_id = ?component_request_id
                                    ))
                                    .instrument(current_span),
                                );
                            } else {
                                warn!(
                                    component_index,
                                    "Component requested successor but it's the last in chain"
                                );
                                component_response_cx
                                    .respond_with_error(jsonrpcmsg::Error::internal_error())?;
                            }
                        }

                        ConductorMessage::ComponentToItsSuccessorSendNotification {
                            component_index,
                            args: scp::ToSuccessorNotification { method, params },
                            component_cx,
                        } => {
                            let successor_index = component_index + 1;
                            if let Some(successor_component) = self.components.get(successor_index)
                            {
                                debug!(
                                    component_index,
                                    successor_index,
                                    method = %method,
                                    "Routing _proxy/successor/notification to successor component"
                                );
                                successor_component
                                    .jsonrpccx
                                    .send_json_notification(method, params)?
                            } else {
                                warn!(
                                    component_index,
                                    "Component sent successor notification but it's the last in chain"
                                );
                                component_cx
                                    .send_error_notification(jsonrpcmsg::Error::internal_error())?;
                            }
                        }

                        ConductorMessage::ResponseReceived {
                            mut result,
                            response_cx,
                            method,
                            from_last_component,
                        } => {
                            debug!(method, from_last_component, "Forwarding response received from component");

                            // If this is an InitializeResponse from the last component (agent),
                            // check for mcp_acp_transport capability
                            if from_last_component && method == "initialize" {
                                if let Ok(ref mut response_value) = result {
                                    if let Ok(mut init_response) = serde_json::from_value::<agent_client_protocol::InitializeResponse>(response_value.clone()) {
                                        // Check if agent has mcp_acp_transport capability
                                        let has_capability = init_response.has_meta_capability(scp::McpAcpTransport);
                                        self.agent_needs_mcp_bridging = Some(!has_capability);

                                        info!(
                                            has_capability,
                                            agent_needs_mcp_bridging = !has_capability,
                                            "Detected agent MCP capability from InitializeResponse"
                                        );

                                        // Add the capability if agent doesn't have it
                                        if !has_capability {
                                            init_response = init_response.add_meta_capability(scp::McpAcpTransport);
                                            *response_value = serde_json::to_value(&init_response).unwrap();
                                            info!("Added mcp_acp_transport capability to agent's InitializeResponse");
                                        }
                                    }
                                }
                            }

                            if let Err(error) = response_cx.respond_with_result(result) {
                                error!(?error, "Failed to forward response");
                            } else {
                                debug!("Successfully forwarded response");
                            }
                        }

                        ConductorMessage::SuccessorResponseReceived {
                            mut result,
                            component_response_cx,
                            method,
                            from_last_component,
                        } => {
                            debug!(method, from_last_component, "Processing successor response");

                            // If this is an InitializeResponse from the last component (agent),
                            // check for mcp_acp_transport capability
                            if from_last_component && method == "initialize" {
                                if let Ok(ref mut response_value) = result {
                                    if let Ok(mut init_response) = serde_json::from_value::<agent_client_protocol::InitializeResponse>(response_value.clone()) {
                                        // Check if agent has mcp_acp_transport capability
                                        let has_capability = init_response.has_meta_capability(scp::McpAcpTransport);
                                        self.agent_needs_mcp_bridging = Some(!has_capability);

                                        info!(
                                            has_mcp_acp_transport = has_capability,
                                            agent_needs_mcp_bridging = !has_capability,
                                            "Detected agent MCP bridging capability"
                                        );

                                        // Add capability if not present so earlier components see it
                                        if !has_capability {
                                            init_response = init_response.add_meta_capability(scp::McpAcpTransport);
                                            *response_value = serde_json::to_value(&init_response).unwrap();
                                            info!("Added mcp_acp_transport capability to response");
                                        }
                                    }
                                }
                            }

                            // Forward the (possibly modified) response
                            if let Err(error) = component_response_cx
                                .respond(scp::ToSuccessorResponse::from(result))
                            {
                                error!(?error, "Failed to forward successor response");
                            } else {
                                debug!("Successfully forwarded successor response");
                            }
                        }

                        ConductorMessage::Error { error } => {
                            error!(
                                error_code = error.code,
                                error_message = %error.message,
                                "Error in spawned task"
                            );
                        }
                    };
                }
                Ok(())
            })
            .await
            .map_err(|err| anyhow::anyhow!("{err:?}"))
    }

    /// Transforms MCP servers with `acp:$UUID` URLs for agents that need bridging.
    ///
    /// For each MCP server with an `acp:` URL:
    /// 1. Spawns a TCP listener on an ephemeral port
    /// 2. Stores the mapping for message routing
    /// 3. Transforms the server to use stdio transport pointing to `conductor mcp $PORT`
    ///
    /// Returns the modified NewSessionRequest with transformed MCP servers.
    async fn transform_mcp_servers(&mut self, mut request: NewSessionRequest) -> NewSessionRequest {
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
                    match self.spawn_tcp_listener(url.clone()).await {
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

    /// Spawns a TCP listener for an MCP bridge and stores the mapping.
    ///
    /// Binds to `localhost:0` to get an ephemeral port, then stores the
    /// `acp_url → tcp_port` mapping in `self.mcp_bridges`.
    ///
    /// Returns the bound port number.
    async fn spawn_tcp_listener(&mut self, acp_url: String) -> anyhow::Result<u16> {
        use tokio::net::TcpListener;

        // Bind to ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let tcp_port = listener.local_addr()?.port();

        info!(
            acp_url = acp_url,
            tcp_port, "Bound TCP listener for MCP bridge"
        );

        // Store mapping for message routing (Phase 3)
        self.mcp_bridges.insert(
            acp_url.clone(),
            McpBridgeInfo {
                acp_url: acp_url.clone(),
                tcp_port,
            },
        );

        // TODO Phase 2b: Accept connections from `conductor mcp $PORT`
        // For now, just drop the listener - we'll implement connection handling later
        drop(listener);

        Ok(tcp_port)
    }
}

fn ignore_send_err<T>(_: Result<T, mpsc::SendError>) {}

async fn send_request_and_forward_response<Req: JsonRpcRequest<Response = serde_json::Value>>(
    to_cx: &JsonRpcCx,
    req: Req,
    response_cx: JsonRpcRequestCx<serde_json::Value>,
    mut conductor_tx: mpsc::Sender<ConductorMessage>,
    method: String,
    from_last_component: bool,
) {
    let response = to_cx.send_request(req);
    let request_id = response_cx.id().clone();
    let current_span = tracing::Span::current();
    tokio::task::spawn_local(
        async move {
            debug!("Waiting for response");
            let result = response.recv().await;
            let is_ok = result.is_ok();
            debug!(is_ok, ?result, "Received response, sending to conductor");
            if let Err(error) = conductor_tx
                .send(ConductorMessage::ResponseReceived {
                    result,
                    response_cx,
                    method,
                    from_last_component,
                })
                .await
            {
                error!(?error, "Failed to send response to conductor");
            } else {
                debug!("Sent response to conductor for forwarding");
            }
        }
        .instrument(tracing::info_span!("receive_response", request_id = ?request_id))
        .instrument(current_span),
    );
}

struct SuccessorSendCallbacks {
    component_index: usize,
    conductor_tx: mpsc::Sender<ConductorMessage>,
}

impl scp::ConductorCallbacks for SuccessorSendCallbacks {
    async fn successor_send_request(
        &mut self,
        args: scp::ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<scp::ToSuccessorResponse<serde_json::Value>>,
    ) -> Result<(), agent_client_protocol::Error> {
        self.conductor_tx
            .send(ConductorMessage::ComponentToItsSuccessorSendRequest {
                component_index: self.component_index,
                args,
                component_response_cx: response,
            })
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)
    }

    async fn successor_send_notification(
        &mut self,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        cx: &scp::JsonRpcCx,
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
    ComponentToItsClientMessage {
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
        component_response_cx: JsonRpcRequestCx<scp::ToSuccessorResponse<serde_json::Value>>,
    },

    /// Notification from a component to its successor via `_proxy/successor/notification`.
    ///
    /// Similar to requests, but no response is expected. The conductor strips
    /// the prefix and forwards to the next component.
    ComponentToItsSuccessorSendNotification {
        component_index: usize,
        args: scp::ToSuccessorNotification<serde_json::Value>,
        component_cx: JsonRpcCx,
    },

    /// Response received from a request that needs to be forwarded.
    ///
    /// Responses are routed back through the conductor to enable centralized
    /// inspection and modification (e.g., adding capabilities to InitializeResponse).
    ResponseReceived {
        /// The response result (Ok with JSON value or Err with error)
        result: Result<serde_json::Value, jsonrpcmsg::Error>,
        /// Context to send the response to
        response_cx: JsonRpcRequestCx<serde_json::Value>,
        /// The method that was called (e.g., "initialize")
        method: String,
        /// Whether this response is from the last component (the agent)
        from_last_component: bool,
    },

    /// Response received from a successor component that needs to be forwarded.
    ///
    /// Similar to ResponseReceived but for responses from _proxy/successor/* requests.
    SuccessorResponseReceived {
        /// The response result (Ok with JSON value or Err with error)
        result: Result<serde_json::Value, jsonrpcmsg::Error>,
        /// Context to send the response to (wrapped in ToSuccessorResponse)
        component_response_cx: JsonRpcRequestCx<scp::ToSuccessorResponse<serde_json::Value>>,
        /// The method that was called (e.g., "initialize")
        method: String,
        /// Whether this response is from the last component (the agent)
        from_last_component: bool,
    },

    /// Error from a spawned task that couldn't be handled locally.
    ///
    /// Currently logged as a warning. Future versions may trigger chain shutdown.
    Error { error: jsonrpcmsg::Error },
}
