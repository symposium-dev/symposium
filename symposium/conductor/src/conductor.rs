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

use std::pin::Pin;

use agent_client_protocol::{ClientRequest, InitializeRequest};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt, channel::mpsc};
use scp::{
    AcpAgentToClientMessages, AcpClientToAgentMessages, JsonRpcConnection, JsonRpcCx,
    JsonRpcNotification, JsonRpcRequest, JsonRpcRequestCx, ProxyToConductorMessages,
};
use serde_json::json;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use crate::component::Component;

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
    mut request: InitializeRequest,
    component_index: usize,
    total_components: usize,
) -> InitializeRequest {
    let is_last_component = component_index == total_components - 1;

    if is_last_component {
        // Last component - remove proxy capability if present
        if let Some(ref mut meta) = request.meta {
            if let Some(obj) = meta.as_object_mut() {
                if let Some(symposium) = obj.get_mut("symposium") {
                    if let Some(symposium_obj) = symposium.as_object_mut() {
                        symposium_obj.remove("proxy");
                    }
                }
            }
        }
    } else {
        // Has successor - add proxy capability
        let mut meta = request.meta.take().unwrap_or(json!({}));

        if let Some(obj) = meta.as_object_mut() {
            let symposium = obj.entry("symposium").or_insert_with(|| json!({}));

            if let Some(symposium_obj) = symposium.as_object_mut() {
                symposium_obj.insert("version".to_string(), json!("1.0"));
                symposium_obj.insert("proxy".to_string(), json!(true));
            }
        }

        request.meta = Some(meta);
    }

    request
}

/// The conductor manages the proxy chain lifecycle and message routing.
///
/// It maintains connections to all components in the chain and routes messages
/// bidirectionally between the editor, components, and agent.
///
/// # Type Parameters
///
/// - `OB`: Outgoing byte stream (to editor)
/// - `IB`: Incoming byte stream (from editor)
pub struct Conductor<OB: AsyncWrite, IB: AsyncRead> {
    /// Stream for sending messages back to the editor
    outgoing_bytes: OB,
    /// Stream for receiving messages from the editor
    incoming_bytes: IB,
    /// Channel for receiving internal conductor messages from spawned tasks
    conductor_rx: mpsc::Receiver<ConductorMessage>,
    /// The chain of spawned components, ordered from first (index 0) to last
    components: Vec<Component>,
}

impl<OB: AsyncWrite, IB: AsyncRead> Conductor<OB, IB> {
    pub async fn run(
        outgoing_bytes: OB,
        incoming_bytes: IB,
        mut proxies: Vec<String>,
    ) -> anyhow::Result<()> {
        if proxies.len() == 0 {
            anyhow::bail!("must have at least one component")
        }

        info!(
            component_count = proxies.len(),
            components = ?proxies,
            "Starting conductor with component chain"
        );

        proxies.reverse();
        let (conductor_tx, conductor_rx) = mpsc::channel(128 /* chosen arbitrarily */);

        tokio::task::LocalSet::new()
            .run_until(async move {
                Conductor {
                    outgoing_bytes,
                    incoming_bytes,
                    components: Default::default(),
                    conductor_rx,
                }
                .launch_proxy(proxies, conductor_tx)
                .await
            })
            .await
    }

    /// Recursively spawns components and builds the proxy chain.
    ///
    /// This function implements the recursive chain building pattern:
    /// 1. Pop the next component from the `proxies` list
    /// 2. Spawn it as a subprocess with stdio communication
    /// 3. Set up JSON-RPC connection and message handlers
    /// 4. Recursively call itself to spawn the next component
    /// 5. When no components remain, start the message routing loop via `serve()`
    ///
    /// Each component is given a channel to send messages back to the conductor,
    /// enabling the bidirectional message routing.
    ///
    /// # Arguments
    ///
    /// - `proxies`: Stack of component commands to spawn (reversed, so we pop from the end)
    /// - `conductor_tx`: Channel for components to send messages back to conductor
    fn launch_proxy(
        mut self,
        mut proxies: Vec<String>,
        conductor_tx: mpsc::Sender<ConductorMessage>,
    ) -> Pin<Box<impl Future<Output = anyhow::Result<()>>>> {
        Box::pin(async move {
            let Some(next_proxy) = proxies.pop() else {
                info!("All components spawned, starting message routing");
                return self.serve(conductor_tx).await;
            };

            let component_index = self.components.len();
            let remaining = proxies.len();

            info!(
                component_index,
                component_name = %next_proxy,
                remaining_components = remaining,
                "Spawning component"
            );

            let mut child = tokio::process::Command::new(&next_proxy)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            // Take ownership of the streams (can only do this once!)
            let stdin = child.stdin.take().expect("Failed to open stdin");
            let stdout = child.stdout.take().expect("Failed to open stdout");

            debug!(
                component_index,
                component_name = %next_proxy,
                "Component process spawned, setting up JSON-RPC connection"
            );

            JsonRpcConnection::new(stdin.compat_write(), stdout.compat())
                // The proxy can send *editor* messages to use
                .on_receive(AcpAgentToClientMessages::send_to({
                    let mut conductor_tx = conductor_tx.clone();
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
                    conductor_tx: conductor_tx.clone(),
                }))
                .with_client(async move |jsonrpccx| {
                    self.components.push(Component { child, jsonrpccx });
                    self.launch_proxy(proxies, conductor_tx)
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
    /// - `conductor_tx`: Channel for spawned tasks to send messages back to this loop
    async fn serve(mut self, conductor_tx: mpsc::Sender<ConductorMessage>) -> anyhow::Result<()> {
        JsonRpcConnection::new(self.outgoing_bytes, self.incoming_bytes)
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
                                let method = init_req.method();
                                let total_components = self.components.len();
                                let has_successor = total_components > 1;

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
                                    json_rpc_request_cx,
                                    conductor_tx.clone(),
                                )
                                .await;
                            }

                            scp::AcpClientToAgentMessage::Request(
                                client_request,
                                json_rpc_request_cx,
                            ) => {
                                let method = client_request.method();
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
                                    debug!(
                                        component_index,
                                        method = agent_request.method(),
                                        target,
                                        "Routing component request to its client"
                                    );
                                    send_request_and_forward_response(
                                        its_client,
                                        agent_request,
                                        json_rpc_request_cx,
                                        conductor_tx.clone(),
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
                                    its_client.send_notification(agent_notification)?;
                                }
                            }
                        }

                        ConductorMessage::ComponentToItsSuccessorSendRequest {
                            component_index,
                            args: scp::ToSuccessorRequest { method, params },
                            component_response_cx,
                        } => {
                            let successor_index = component_index + 1;
                            if let Some(successor_component) = self.components.get(successor_index)
                            {
                                let is_last_component = successor_index == self.components.len() - 1;

                                debug!(
                                    component_index,
                                    successor_index,
                                    method = %method,
                                    is_last_component,
                                    "Routing _proxy/successor/request to successor component"
                                );

                                // Special handling for initialize: manage proxy capability based on successor position
                                let (final_method, final_params) = if method == "initialize" {
                                    // Try to parse params as InitializeRequest
                                    if let Ok(init_req) = serde_json::from_value::<InitializeRequest>(params.clone()) {
                                        let total_components = self.components.len();
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
                                } else {
                                    (method, params)
                                };

                                let successor_response = successor_component
                                    .jsonrpccx
                                    .send_json_request(final_method, final_params);

                                let mut conductor_tx = conductor_tx.clone();
                                tokio::task::spawn_local(async move {
                                    let v = successor_response.recv().await;
                                    if let Err(error) = component_response_cx
                                        .respond(scp::ToSuccessorResponse::from(v))
                                    {
                                        ignore_send_err(
                                            conductor_tx
                                                .send(ConductorMessage::Error { error })
                                                .await,
                                        );
                                    }
                                });
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
}

fn ignore_send_err<T>(_: Result<T, mpsc::SendError>) {}

async fn send_request_and_forward_response<Req: JsonRpcRequest>(
    to_cx: &JsonRpcCx,
    req: Req,
    response_cx: JsonRpcRequestCx<Req::Response>,
    mut conductor_tx: mpsc::Sender<ConductorMessage>,
) {
    let response = to_cx.send_request(req);
    tokio::task::spawn_local(async move {
        if let Err(error) = response_cx.respond_with_result(response.recv().await) {
            ignore_send_err(conductor_tx.send(ConductorMessage::Error { error }).await);
        }
    });
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

    /// Error from a spawned task that couldn't be handled locally.
    ///
    /// Currently logged as a warning. Future versions may trigger chain shutdown.
    Error { error: jsonrpcmsg::Error },
}
