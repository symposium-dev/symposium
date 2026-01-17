//! Configuration Agent for Symposium.
//!
//! The ConfigAgent manages sessions and routes them to conductors based on configuration.
//! It handles:
//! - Initial setup when no config exists
//! - Runtime configuration via `/symposium:config` command
//! - Delegating sessions to appropriate conductors

mod conductor_actor;
mod config_mode_actor;
mod uberconductor_actor;

use crate::user_config::SymposiumUserConfig;
use conductor_actor::ConductorHandle;
use config_mode_actor::{ConfigModeHandle, ConfigModeOutput};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use fxhash::FxHashMap;
use sacp::link::AgentToClient;
use sacp::schema::{
    AgentCapabilities, AvailableCommand, ContentBlock, ContentChunk, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionId, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use sacp::util::MatchMessage;
use sacp::{ClientPeer, Component, JrConnectionCx, JrRequestCx, MessageCx};
use std::path::PathBuf;
use uberconductor_actor::UberconductorHandle;

/// The slash command name for entering config mode.
const CONFIG_SLASH_COMMAND: &str = "symposium:config";

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
    /// - `actor`: Handle to the config mode actor
    /// - `return_to`: If Some, this config session was spawned from `/symposium:config`
    ///   and should return to the original session when done
    Config {
        actor: ConfigModeHandle,
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
        tx: futures::channel::mpsc::UnboundedSender<ConfigAgentMessage>,
        cx: JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        while let Some(message) = rx.next().await {
            match message {
                ConfigAgentMessage::MessageFromClient(message) => {
                    self.handle_message_from_client(message, &uberconductor, &cx, &tx)
                        .await?
                }

                ConfigAgentMessage::MessageToClient(message) => {
                    self.handle_message_to_client(message, &cx).await?;
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

                ConfigAgentMessage::ConfigModeOutput(session_id, output) => {
                    self.handle_config_mode_output(session_id, output, &cx)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Handle output from a config mode actor.
    async fn handle_config_mode_output(
        &mut self,
        session_id: SessionId,
        output: ConfigModeOutput,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        match output {
            ConfigModeOutput::SendMessage(text) => {
                cx.send_notification(SessionNotification::new(
                    session_id,
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into())),
                ))?;
            }

            ConfigModeOutput::Done { config } => {
                // Save the configuration
                if let Err(e) = config.save() {
                    cx.send_notification(SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            format!("Error saving configuration: {}", e).into(),
                        )),
                    ))?;
                }

                // Get the return_to conductor if any
                let return_to = match self.sessions.get(&session_id) {
                    Some(SessionState::Config { return_to, .. }) => return_to.clone(),
                    _ => None,
                };

                if let Some(conductor) = return_to {
                    // Return to the previous session
                    self.sessions
                        .insert(session_id.clone(), SessionState::Delegating { conductor });
                    cx.send_notification(SessionNotification::new(
                        session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            "Configuration saved. Returning to your session.".into(),
                        )),
                    ))?;
                } else {
                    // No session to return to - this was initial setup or standalone
                    // For now, just remove the session
                    self.sessions.remove(&session_id);
                    cx.send_notification(SessionNotification::new(
                        session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            "Configuration saved. Please start a new session.".into(),
                        )),
                    ))?;
                }
            }

            ConfigModeOutput::Cancelled => {
                // Get the return_to conductor if any
                let return_to = match self.sessions.get(&session_id) {
                    Some(SessionState::Config { return_to, .. }) => return_to.clone(),
                    _ => None,
                };

                if let Some(conductor) = return_to {
                    // Return to the previous session without saving
                    self.sessions
                        .insert(session_id.clone(), SessionState::Delegating { conductor });
                    cx.send_notification(SessionNotification::new(
                        session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            "Configuration cancelled. Returning to your session.".into(),
                        )),
                    ))?;
                } else {
                    // No session to return to
                    self.sessions.remove(&session_id);
                    cx.send_notification(SessionNotification::new(
                        session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            "Configuration cancelled.".into(),
                        )),
                    ))?;
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

    /// Enter configuration mode for a session.
    ///
    /// If the session was delegating to a conductor, we store the conductor handle
    /// so we can return to it when config mode exits.
    async fn enter_config_mode(
        &mut self,
        session_id: SessionId,
        request_cx: JrRequestCx<PromptResponse>,
        cx: &JrConnectionCx<AgentToClient>,
        config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    ) -> Result<(), sacp::Error> {
        // Get the current session state to potentially preserve the conductor
        let return_to = match self.sessions.get(&session_id) {
            Some(SessionState::Delegating { conductor }) => Some(conductor.clone()),
            _ => None,
        };

        // Pause the conductor if we have one - it will resume when config mode exits
        let resume_tx = if let Some(ref conductor) = return_to {
            Some(conductor.pause().await?)
        } else {
            None
        };

        // Load current config (or start with empty agent)
        let current_config = self
            .load_config()
            .map_err(|e| sacp::Error::new(-32603, e.to_string()))?
            .unwrap_or_else(|| SymposiumUserConfig::with_agent(""));

        // Spawn the config mode actor (it holds resume_tx and drops it on exit)
        let actor_handle = ConfigModeHandle::spawn(
            current_config,
            session_id.clone(),
            config_agent_tx.clone(),
            resume_tx,
            cx,
        )?;

        // Transition to config state
        self.sessions.insert(
            session_id,
            SessionState::Config {
                actor: actor_handle,
                return_to,
            },
        );

        // Respond to the prompt (the actor will send the welcome message)
        request_cx.respond(PromptResponse::new(StopReason::EndTurn))
    }

    /// Check if the prompt is invoking the config slash command.
    fn is_config_command(request: &PromptRequest) -> bool {
        // Extract text from the prompt
        let text: String = request
            .prompt
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        let text = text.trim();

        // Check for /symposium:config (with or without the leading slash)
        text == format!("/{}", CONFIG_SLASH_COMMAND) || text == CONFIG_SLASH_COMMAND
    }

    /// Handle a prompt request.
    async fn handle_prompt(
        &mut self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
        cx: &JrConnectionCx<AgentToClient>,
        config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    ) -> Result<(), sacp::Error> {
        let session_id = request.session_id.clone();

        // Get the session state
        let session_state = self.sessions.get(&session_id).cloned();

        match session_state {
            Some(SessionState::Delegating { conductor }) => {
                // Check if this is the config command
                if Self::is_config_command(&request) {
                    self.enter_config_mode(session_id, request_cx, cx, config_agent_tx)
                        .await
                } else {
                    // Forward to conductor
                    conductor.send_prompt(request, request_cx).await
                }
            }
            Some(SessionState::InitialSetup) => {
                // Handle initial setup input (not yet using actor)
                self.handle_config_input(request, request_cx, cx).await
            }
            Some(SessionState::Config { actor, .. }) => {
                // Forward input to the config mode actor
                let input = Self::extract_prompt_text(&request);
                if actor.send_input(input).await.is_err() {
                    return request_cx
                        .respond_with_error(sacp::Error::new(-32603, "Config mode actor closed"));
                }
                request_cx.respond(PromptResponse::new(StopReason::EndTurn))
            }
            None => {
                // Unknown session
                request_cx.respond_with_error(sacp::Error::new(-32600, "Unknown session"))
            }
        }
    }

    /// Extract text content from a prompt request.
    fn extract_prompt_text(request: &PromptRequest) -> String {
        request
            .prompt
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
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

    /// Handle messages from conductors destined for the client.
    ///
    /// Intercepts `AvailableCommandsUpdate` to inject the `/symposium:config` command,
    /// then forwards the message to the client.
    async fn handle_message_to_client(
        &self,
        message: MessageCx,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        MatchMessage::new(message)
            .if_notification(async |mut notif: SessionNotification| {
                // Check if this is an AvailableCommandsUpdate
                if let SessionUpdate::AvailableCommandsUpdate(ref mut update) = notif.update {
                    // Inject the /symposium:config command
                    update.available_commands.push(AvailableCommand::new(
                        CONFIG_SLASH_COMMAND,
                        "Configure Symposium settings",
                    ));
                }
                // Forward the (possibly modified) notification
                cx.send_notification(notif)
            })
            .await
            .otherwise(async |message| {
                // Default: proxy the message onward
                cx.send_proxied_message_to(ClientPeer, message)
            })
            .await
    }

    /// Handle messages using MatchMessage for dispatch.
    async fn handle_message_from_client(
        &mut self,
        message: MessageCx,
        uberconductor: &UberconductorHandle,
        cx: &JrConnectionCx<AgentToClient>,
        config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
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
                    self.handle_prompt(request, request_cx, cx, config_agent_tx)
                        .await
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

    /// Output from a config mode actor.
    ConfigModeOutput(SessionId, ConfigModeOutput),
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
        let tx_for_run = tx.clone();
        AgentToClient::builder()
            .with_spawned({
                async move |cx| {
                    // Create the uberconductor actor
                    let uberconductor = UberconductorHandle::spawn(trace_dir, tx.clone(), &cx)?;
                    self.run(uberconductor, rx, tx_for_run, cx).await
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
