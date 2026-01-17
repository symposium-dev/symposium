//! Configuration Agent for Symposium.
//!
//! The ConfigAgent manages sessions and routes them to conductors based on configuration.
//! It handles:
//! - Initial setup when no config exists
//! - Runtime configuration via `/symposium:config` command
//! - Delegating sessions to appropriate conductors

mod conductor_actor;
mod uberconductor_actor;

use crate::user_config::SymposiumUserConfig;
use conductor_actor::ConductorHandle;
use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::StreamExt;
use fxhash::FxHashMap;
use sacp::link::AgentToClient;
use sacp::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent,
};
use sacp::util::MatchMessage;
use sacp::{ClientPeer, Component, JrConnectionCx, JrRequestCx, MessageCx};
use std::path::PathBuf;
use uberconductor_actor::UberconductorHandle;

/// Extract the session ID from a message, if present.
fn get_session_id(message: &MessageCx) -> Result<Option<SessionId>, sacp::Error> {
    let params = match message {
        MessageCx::Request(req, _) => req.params(),
        MessageCx::Notification(notif) => notif.params(),
    };
    match params.get("sessionId") {
        Some(value) => {
            let session_id = serde_json::from_value(value.clone())?;
            Ok(Some(session_id))
        }
        None => Ok(None),
    }
}

/// State for a single session.
#[derive(Clone)]
enum SessionState {
    /// User is in configuration mode.
    ///
    /// - `current_config`: The configuration being edited (starts from disk or default)
    /// - `return_to`: If Some, this config session was spawned from `/symposium:config`
    ///   and should return to the original session when done
    Config {
        current_config: SymposiumUserConfig,
        return_to: Option<ConductorHandle>,
    },

    /// Session is delegated to a conductor.
    Delegating { conductor: ConductorHandle },

    /// Initial setup - no configuration exists yet.
    /// After setup completes, transitions to Config with return_to: None.
    InitialSetup,
}

/// The ConfigAgent manages sessions and configuration.
///
/// It implements `Component<AgentToClient>` and routes sessions to conductors
/// based on the current configuration.
pub struct ConfigAgent {
    /// Session states, keyed by session ID.
    sessions: FxHashMap<SessionId, SessionState>,

    /// Custom config path for testing. If None, uses the default path.
    config_path: Option<PathBuf>,

    /// Trace directory for conductors.
    trace_dir: Option<PathBuf>,
}

impl ConfigAgent {
    /// Create a new ConfigAgent.
    pub fn new() -> Self {
        Self {
            sessions: Default::default(),
            config_path: None,
            trace_dir: None,
        }
    }

    /// Set a custom config path (for testing).
    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Set trace directory for conductors.
    pub fn with_trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.trace_dir = Some(dir.into());
        self
    }

    /// Load configuration from disk.
    fn load_config(&self) -> anyhow::Result<Option<SymposiumUserConfig>> {
        SymposiumUserConfig::load(self.config_path.as_ref())
    }

    /// The main "config agent method"
    async fn run(
        mut self,
        uberconductor: UberconductorHandle,
        mut rx: UnboundedReceiver<ConfigAgentMessage>,
        cx: JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        while let Some(message) = rx.next().await {
            match message {
                ConfigAgentMessage::MessageFromClient(message) => {
                    self.handle_message_from_client(message, &uberconductor, &cx)
                        .await?
                }

                ConfigAgentMessage::MessageToClient(message) => {
                    // Forward message from conductor to client
                    cx.send_proxied_message_to(ClientPeer, message)?;
                }

                ConfigAgentMessage::NewSessionCreated(response, conductor, request_cx) => {
                    // Store the session mapping before responding
                    self.sessions.insert(
                        response.session_id.clone(),
                        SessionState::Delegating { conductor },
                    );
                    // Now respond to the client
                    request_cx.respond(response)?;
                }
            }
        }
        Ok(())
    }

    /// Handle a new session request.
    async fn handle_new_session(
        &mut self,
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
        uberconductor: &UberconductorHandle,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        // Load configuration
        let config = self
            .load_config()
            .map_err(|e| sacp::Error::new(-32603, e.to_string()))?;

        match config {
            None => {
                // No config - start initial setup
                let session_id = SessionId::new(uuid::Uuid::new_v4().to_string());

                self.sessions
                    .insert(session_id.clone(), SessionState::InitialSetup);

                // Respond with our generated session ID
                request_cx.respond(NewSessionResponse::new(session_id.clone()))?;

                // Send welcome message
                let welcome = self.initial_setup_welcome();
                cx.send_notification(SessionNotification::new(
                    session_id,
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(welcome.into())),
                ))?;

                Ok(())
            }
            Some(config) => {
                // Send to uberconductor - it will get/create a conductor and forward the request.
                // The conductor will send NewSessionCreated back to us when done.
                uberconductor.new_session(config, request, request_cx).await
            }
        }
    }

    /// Handle a prompt request.
    async fn handle_prompt(
        &mut self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        let session_id = request.session_id.clone();

        // Get the session state
        let session_state = self.sessions.get(&session_id).cloned();

        match session_state {
            Some(SessionState::Delegating { conductor }) => {
                // Forward to conductor
                conductor.send_prompt(request, request_cx).await
            }
            Some(SessionState::InitialSetup) | Some(SessionState::Config { .. }) => {
                // Handle config input
                self.handle_config_input(request, request_cx, cx).await
            }
            None => {
                // Unknown session
                request_cx.respond_with_error(sacp::Error::new(-32600, "Unknown session"))
            }
        }
    }

    /// Handle input during configuration mode.
    async fn handle_config_input(
        &mut self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        let session_id = request.session_id.clone();

        // Extract text from prompt
        let input = request
            .prompt
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(TextContent { text, .. }) => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        // TODO: Implement config state machine (agent selection, etc.)
        // For now, just echo back that we received input
        let response = format!(
            "Received: {}\n\n(Config mode not yet fully implemented)",
            input
        );

        cx.send_notification(SessionNotification::new(
            session_id,
            SessionUpdate::AgentMessageChunk(ContentChunk::new(response.into())),
        ))?;

        request_cx.respond(PromptResponse::new(StopReason::EndTurn))
    }

    /// Handle a message for an existing session by forwarding to the appropriate conductor.
    async fn handle_session_message(
        &self,
        session_id: &SessionId,
        message: MessageCx,
    ) -> Result<(), sacp::Error> {
        match self.sessions.get(session_id) {
            Some(SessionState::Delegating { conductor }) => {
                conductor.forward_message(message).await
            }
            Some(SessionState::InitialSetup) | Some(SessionState::Config { .. }) => {
                // Config mode doesn't handle arbitrary messages
                match message {
                    MessageCx::Request(req, request_cx) => {
                        request_cx.respond_with_error(sacp::Error::new(
                            -32601,
                            format!("Method not supported in config mode: {}", req.method()),
                        ))
                    }
                    MessageCx::Notification(_) => Ok(()),
                }
            }
            None => match message {
                MessageCx::Request(_, request_cx) => {
                    request_cx.respond_with_error(sacp::Error::new(-32600, "Unknown session"))
                }
                MessageCx::Notification(_) => Ok(()),
            },
        }
    }

    /// Generate the initial setup welcome message.
    fn initial_setup_welcome(&self) -> String {
        "Welcome to Symposium!\n\n\
         No configuration found. Let's set up your AI agent.\n\n\
         (Initial setup wizard coming soon...)"
            .to_string()
    }

    /// Handle messages using MatchMessage for dispatch.
    async fn handle_message_from_client(
        &mut self,
        message: MessageCx,
        uberconductor: &UberconductorHandle,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        MatchMessage::new(message)
            .if_request(
                async |_init: InitializeRequest, request_cx: JrRequestCx<InitializeResponse>| {
                    request_cx.respond(
                        InitializeResponse::new(sacp::schema::ProtocolVersion::LATEST)
                            .agent_capabilities(AgentCapabilities::new()),
                    )
                },
            )
            .await
            .if_request(
                async |request: NewSessionRequest, request_cx: JrRequestCx<NewSessionResponse>| {
                    self.handle_new_session(request, request_cx, uberconductor, cx)
                        .await
                },
            )
            .await
            .if_request(
                async |request: PromptRequest, request_cx: JrRequestCx<PromptResponse>| {
                    self.handle_prompt(request, request_cx, cx).await
                },
            )
            .await
            .otherwise(async |message| {
                // For messages that contain a session ID, look up the session and forward
                if let Some(session_id) = get_session_id(&message)? {
                    return self.handle_session_message(&session_id, message).await;
                }

                // No session ID - return error for requests, ignore notifications
                tracing::debug!("Received message without session ID: {:?}", message);
                match message {
                    MessageCx::Request(req, request_cx) => request_cx.respond_with_error(
                        sacp::Error::new(-32601, format!("Method not found: {}", req.method())),
                    ),
                    MessageCx::Notification(_) => Ok(()),
                }
            })
            .await
    }
}

pub enum ConfigAgentMessage {
    /// Sent when a client sends a message to the agent.
    MessageFromClient(MessageCx),

    /// Sent when a conductor wants to send a message to the client.
    /// ConfigAgent forwards this (and can inject additional messages like session updates).
    MessageToClient(MessageCx),

    /// Sent when a conductor has established a session.
    /// ConfigAgent stores the session mapping, then responds to the client.
    NewSessionCreated(
        NewSessionResponse,
        ConductorHandle,
        JrRequestCx<NewSessionResponse>,
    ),
}

impl Default for ConfigAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component<AgentToClient> for ConfigAgent {
    async fn serve(
        self,
        client: impl Component<sacp::link::ClientToAgent>,
    ) -> Result<(), sacp::Error> {
        let (tx, rx) = unbounded();
        let trace_dir = self.trace_dir.clone();
        let tx_for_message = tx.clone();
        AgentToClient::builder()
            .with_spawned({
                async move |cx| {
                    // Create the uberconductor actor
                    let uberconductor = UberconductorHandle::spawn(trace_dir, tx.clone(), &cx)?;
                    self.run(uberconductor, rx, cx).await
                }
            })
            .on_receive_message(
                async move |message: MessageCx, _cx: JrConnectionCx<AgentToClient>| {
                    tx_for_message
                        .unbounded_send(ConfigAgentMessage::MessageFromClient(message))
                        .map_err(|_| sacp::util::internal_error("no config-agent receiver"))
                },
                sacp::on_receive_message!(),
            )
            .connect_to(client)?
            .serve()
            .await
    }
}
