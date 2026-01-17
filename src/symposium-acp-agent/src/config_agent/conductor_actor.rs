//! Actor that owns and manages a conductor connection.
//!
//! This actor:
//! - Spawns and initializes the conductor
//! - Receives session messages from ConfigAgent via a channel
//! - Forwards messages to the conductor
//! - Forwards notifications from the conductor back to the client

use super::ConfigAgentMessage;
use crate::symposium::SymposiumConfig;
use crate::user_config::SymposiumUserConfig;
use futures::channel::mpsc::UnboundedSender;
use sacp::link::{AgentToClient, ClientToAgent};
use sacp::schema::{
    InitializeRequest, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
};
use sacp::{DynComponent, JrConnectionCx, JrRequestCx, MessageCx};
use sacp_conductor::{Conductor, McpBridgeMode};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Messages that can be sent to the ConductorActor.
pub enum ConductorMessage {
    /// A new session request. The conductor will send NewSessionCreated to ConfigAgent.
    NewSession {
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
    },

    /// A prompt request for a session.
    Prompt {
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
    },

    /// Forward an arbitrary message to the conductor.
    ForwardMessage { message: MessageCx },
}

/// Handle for communicating with a ConductorActor.
#[derive(Clone)]
pub struct ConductorHandle {
    tx: mpsc::Sender<ConductorMessage>,
}

impl ConductorHandle {
    /// Spawn a new conductor actor for the given configuration.
    ///
    /// Returns a handle for sending messages to the actor.
    pub async fn spawn(
        config: &SymposiumUserConfig,
        trace_dir: Option<&PathBuf>,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        client_cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        // Create the channel for receiving messages
        let (tx, rx) = mpsc::channel(32);

        let handle = Self { tx: tx.clone() };

        client_cx.spawn(run_actor(
            config.clone(),
            trace_dir.cloned(),
            config_agent_tx,
            handle.clone(),
            rx,
        ))?;

        Ok(handle)
    }

    /// Send a new session request to the conductor.
    /// The conductor will send NewSessionCreated to ConfigAgent when done.
    pub async fn send_new_session(
        &self,
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
    ) -> Result<(), sacp::Error> {
        self.tx
            .send(ConductorMessage::NewSession {
                request,
                request_cx,
            })
            .await
            .map_err(|_| sacp::util::internal_error("Conductor actor closed"))
    }

    /// Send a prompt request to the conductor.
    pub async fn send_prompt(
        &self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
    ) -> Result<(), sacp::Error> {
        self.tx
            .send(ConductorMessage::Prompt {
                request,
                request_cx,
            })
            .await
            .map_err(|_| sacp::util::internal_error("Conductor actor closed"))
    }

    /// Forward an arbitrary message to the conductor.
    pub async fn forward_message(&self, message: MessageCx) -> Result<(), sacp::Error> {
        self.tx
            .send(ConductorMessage::ForwardMessage { message })
            .await
            .map_err(|_| sacp::util::internal_error("Conductor actor closed"))
    }
}

/// The main actor loop.
async fn run_actor(
    config: SymposiumUserConfig,
    trace_dir: Option<PathBuf>,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    self_handle: ConductorHandle,
    mut rx: mpsc::Receiver<ConductorMessage>,
) -> Result<(), sacp::Error> {
    // Build the symposium config
    let proxy_names = config.enabled_proxies();
    let agent_args = config
        .agent_args()
        .map_err(|e| sacp::Error::new(-32603, e.to_string()))?;

    let mut symposium_config = SymposiumConfig::from_proxy_names(proxy_names);

    if let Some(trace_dir) = trace_dir {
        symposium_config = symposium_config.trace_dir(trace_dir.clone());
    }

    let agent =
        AcpAgent::from_args(&agent_args).map_err(|e| sacp::Error::new(-32603, e.to_string()))?;

    // Build the conductor
    let conductor = Conductor::new_agent(
        "symposium-conductor",
        {
            let symposium_config = symposium_config.clone();
            async move |init_req| {
                tracing::info!(
                    "Building proxy chain with extensions: {:?}",
                    symposium_config.proxy_names()
                );
                let proxies = symposium_config.build_proxies().await?;
                Ok((init_req, proxies, DynComponent::new(agent)))
            }
        },
        McpBridgeMode::default(),
    );

    // Connect to the conductor
    ClientToAgent::builder()
        .on_receive_message(
            async |message_cx: MessageCx, _cx| {
                // Incoming message from the conductor: forward via ConfigAgent to client
                config_agent_tx
                    .unbounded_send(ConfigAgentMessage::MessageToClient(message_cx))
                    .map_err(|_| sacp::util::internal_error("ConfigAgent closed"))
            },
            sacp::on_receive_message!(),
        )
        .run_until(conductor, async |conductor_cx| {
            // Initialize the conductor
            let _init_response = conductor_cx
                .send_request(InitializeRequest::new(
                    sacp::schema::ProtocolVersion::LATEST,
                ))
                .block_task()
                .await?;

            while let Some(message) = rx.recv().await {
                match message {
                    ConductorMessage::NewSession {
                        request,
                        request_cx,
                    } => {
                        let config_agent_tx = config_agent_tx.clone();
                        let self_handle = self_handle.clone();
                        conductor_cx.send_request(request).on_receiving_result(
                            async move |result| {
                                match result {
                                    Ok(response) => {
                                        // Send to ConfigAgent so it can store the session mapping
                                        config_agent_tx
                                            .unbounded_send(ConfigAgentMessage::NewSessionCreated(
                                                response,
                                                self_handle,
                                                request_cx,
                                            ))
                                            .map_err(|_| {
                                                sacp::util::internal_error("ConfigAgent closed")
                                            })
                                    }
                                    Err(e) => {
                                        // Forward error directly to client
                                        request_cx.respond_with_error(e)
                                    }
                                }
                            },
                        )?;
                    }

                    ConductorMessage::Prompt {
                        request,
                        request_cx,
                    } => {
                        if let Err(e) = conductor_cx
                            .send_request(request)
                            .forward_to_request_cx(request_cx)
                        {
                            tracing::error!("Failed to forward prompt to conductor: {}", e);
                        }
                    }

                    ConductorMessage::ForwardMessage { message } => {
                        if let Err(e) =
                            conductor_cx.send_proxied_message_to(sacp::AgentPeer, message)
                        {
                            tracing::error!("Failed to forward message to conductor: {}", e);
                        }
                    }
                }
            }

            tracing::debug!("Conductor actor shutting down");
            Ok(())
        })
        .await
}
