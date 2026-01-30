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

#[cfg(test)]
mod tests;

use crate::recommendations::{Recommendations, RecommendationsExt, WorkspaceRecommendations};
use crate::remote_recommendations;
use crate::user_config::{ConfigPaths, GlobalAgentConfig, WorkspaceModsConfig};
use conductor_actor::ConductorHandle;
use config_mode_actor::{ConfigModeHandle, ConfigModeOutput};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use fxhash::FxHashMap;
use sacp::link::AgentToClient;
use sacp::schema::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, ContentBlock, ContentChunk,
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use sacp::util::MatchMessage;
use sacp::{ClientPeer, Component, JrConnectionCx, JrRequestCx, MessageCx};
use std::path::{Path, PathBuf};
use symposium_recommendations::ComponentSource;
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
    /// - `workspace_path`: The workspace this session is for
    /// - `return_to`: If Some, this config session was spawned from `/symposium:config`
    ///   and should return to the original session when done
    Config {
        actor: ConfigModeHandle,
        workspace_path: PathBuf,
        return_to: Option<ConductorHandle>,
    },

    /// Session is delegated to a conductor.
    Delegating {
        conductor: ConductorHandle,
        workspace_path: PathBuf,
    },
}

/// The ConfigAgent manages sessions and configuration.
///
/// It implements `Component<AgentToClient>` and routes sessions to conductors
/// based on the current configuration.
pub struct ConfigAgent {
    /// Session states, keyed by session ID.
    sessions: FxHashMap<SessionId, SessionState>,

    /// Trace directory for conductors.
    trace_dir: Option<PathBuf>,

    /// Loaded recommendations (from remote + local sources).
    /// None only if loading failed and we're in a degraded state.
    recommendations: Option<Recommendations>,

    /// Configuration paths (where to read/write config files).
    config_paths: ConfigPaths,
}

impl ConfigAgent {
    /// Create a new ConfigAgent using the default config location.
    ///
    /// This loads recommendations from remote sources (with caching fallback).
    /// Returns an error if:
    /// - The config directory cannot be determined
    /// - Recommendations cannot be loaded (no remote access AND no cache)
    pub async fn new() -> anyhow::Result<Self> {
        let config_paths = ConfigPaths::default_location()?;
        let recommendations = remote_recommendations::load_recommendations(&config_paths).await?;
        Ok(Self {
            sessions: Default::default(),
            trace_dir: None,
            recommendations: Some(recommendations),
            config_paths,
        })
    }

    /// Create a new ConfigAgent with custom config paths.
    ///
    /// This loads recommendations from remote sources (with caching fallback).
    /// Useful for integration tests that need real recommendation loading behavior.
    pub async fn with_config_paths_async(config_paths: ConfigPaths) -> anyhow::Result<Self> {
        let recommendations = remote_recommendations::load_recommendations(&config_paths).await?;
        Ok(Self {
            sessions: Default::default(),
            trace_dir: None,
            recommendations: Some(recommendations),
            config_paths,
        })
    }

    /// Create a new ConfigAgent with custom config paths and no recommendation loading.
    ///
    /// Useful for unit tests that will set recommendations via `with_recommendations()`.
    pub fn with_config_paths(config_paths: ConfigPaths) -> Self {
        Self {
            sessions: Default::default(),
            trace_dir: None,
            recommendations: None,
            config_paths,
        }
    }

    /// Set trace directory for conductors.
    pub fn with_trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.trace_dir = Some(dir.into());
        self
    }

    /// Set recommendations (for testing).
    pub fn with_recommendations(mut self, recommendations: Recommendations) -> Self {
        self.recommendations = Some(recommendations);
        self
    }

    /// Load the global agent configuration.
    fn load_global_agent(&self) -> Result<Option<ComponentSource>, sacp::Error> {
        GlobalAgentConfig::load(&self.config_paths)
            .map(|opt| opt.map(|c| c.agent))
            .map_err(|e| sacp::util::internal_error(e.to_string()))
    }

    /// Load workspace mods configuration from disk.
    fn load_mods(&self, workspace_path: &Path) -> Result<Option<WorkspaceModsConfig>, sacp::Error> {
        WorkspaceModsConfig::load(&self.config_paths, workspace_path)
            .map_err(|e| sacp::util::internal_error(e.to_string()))
    }

    /// Get the loaded recommendations.
    ///
    /// Returns None only if the agent was created without loading recommendations
    /// (e.g., via `with_config_paths()` for testing without calling `with_recommendations()`).
    fn load_recommendations(&self) -> Option<&Recommendations> {
        self.recommendations.as_ref()
    }

    /// Load the recommendations for a particular workspace
    fn recommendations_for_workspace(&self, workspace_path: &Path) -> WorkspaceRecommendations {
        self.load_recommendations()
            .map(|r| r.for_workspace(&workspace_path))
            .unwrap_or_default()
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
            tracing::debug!(?message, "ConfigAgent::run: received message");

            match message {
                ConfigAgentMessage::MessageFromClient(message) => {
                    self.handle_message_from_client(message, &uberconductor, &cx, &tx)
                        .await?
                }

                ConfigAgentMessage::MessageToClient(message) => {
                    self.handle_message_to_client(message, &cx).await?;
                }

                ConfigAgentMessage::NewSessionCreated {
                    response,
                    conductor,
                    workspace_path,
                    request_cx,
                } => {
                    let session_id = response.session_id.clone();

                    // Store the session mapping before responding
                    self.sessions.insert(
                        session_id.clone(),
                        SessionState::Delegating {
                            conductor,
                            workspace_path,
                        },
                    );

                    // Respond to the client
                    request_cx.respond(response)?;

                    // Send initial available commands with /symposium:config
                    // This ensures the command is available even if the downstream agent
                    // doesn't send its own AvailableCommandsUpdate
                    cx.send_notification(SessionNotification::new(
                        session_id,
                        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                            AvailableCommand::new(
                                CONFIG_SLASH_COMMAND,
                                "Configure Symposium settings",
                            ),
                        ])),
                    ))?;
                }

                ConfigAgentMessage::ConfigModeOutput(session_id, output) => {
                    self.handle_config_mode_output(session_id, output, &uberconductor, &cx)
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
        _uberconductor: &UberconductorHandle,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        match output {
            ConfigModeOutput::SendMessage(text) => {
                cx.send_notification(SessionNotification::new(
                    session_id,
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into())),
                ))?;
            }

            ConfigModeOutput::Done { agent, mods } => {
                // Get session info (workspace_path and return_to)
                let (workspace_path, return_to) = match self.sessions.get(&session_id) {
                    Some(SessionState::Config {
                        workspace_path,
                        return_to,
                        ..
                    }) => (workspace_path.clone(), return_to.clone()),
                    _ => {
                        tracing::warn!("Config mode done for unknown session: {:?}", session_id);
                        return Ok(());
                    }
                };

                // Save the global agent configuration
                let global_agent_config = GlobalAgentConfig::new(agent);
                if let Err(e) = global_agent_config.save(&self.config_paths) {
                    cx.send_notification(SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            format!("Error saving agent configuration: {}", e).into(),
                        )),
                    ))?;
                }

                // Save the workspace mods configuration
                if let Err(e) = mods.save(&self.config_paths, &workspace_path) {
                    cx.send_notification(SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            format!("Error saving mods configuration: {}", e).into(),
                        )),
                    ))?;
                }

                if let Some(conductor) = return_to {
                    // Return to the previous session
                    self.sessions.insert(
                        session_id.clone(),
                        SessionState::Delegating {
                            conductor,
                            workspace_path,
                        },
                    );
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
                // Note: PendingDiff cancellation is now handled via DiffCancelled message,
                // so we only need to handle regular config mode cancellation here.

                // Get session info (workspace_path and return_to)
                let (workspace_path, return_to) = match self.sessions.get(&session_id) {
                    Some(SessionState::Config {
                        workspace_path,
                        return_to,
                        ..
                    }) => (workspace_path.clone(), return_to.clone()),

                    _ => {
                        tracing::warn!(
                            "Config mode cancelled for unknown session: {:?}",
                            session_id
                        );
                        return Ok(());
                    }
                };

                if let Some(conductor) = return_to {
                    // Return to the previous session without saving
                    self.sessions.insert(
                        session_id.clone(),
                        SessionState::Delegating {
                            conductor,
                            workspace_path,
                        },
                    );
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
                            "Configuration cancelled. Please start a new session.".into(),
                        )),
                    ))?;
                }
            }
        }

        Ok(())
    }

    /// Handle a new session request.
    #[tracing::instrument(skip(self, request_cx, uberconductor, cx, config_agent_tx), ret)]
    async fn handle_new_session(
        &mut self,
        request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
        uberconductor: &UberconductorHandle,
        cx: &JrConnectionCx<AgentToClient>,
        config_agent_tx: &UnboundedSender<ConfigAgentMessage>,
    ) -> Result<(), sacp::Error> {
        tracing::debug!(?request, "handle_new_session");

        // Clone workspace_path upfront so we can move request later
        let workspace_path = request.cwd.clone();

        // Load global agent configuration
        let agent = self.load_global_agent()?;

        // If no global agent, enter initial setup
        let Some(agent) = agent else {
            tracing::debug!("handle_new_session: no global agent configured");

            let session_id = SessionId::new(uuid::Uuid::new_v4().to_string());
            request_cx.respond(NewSessionResponse::new(session_id.clone()))?;

            let workspace_recs = self.recommendations_for_workspace(&workspace_path);

            let actor_handle = ConfigModeHandle::spawn_initial_config(
                workspace_path.clone(),
                self.config_paths.clone(),
                workspace_recs,
                session_id.clone(),
                config_agent_tx.clone(),
                None,
                cx,
            )?;

            self.sessions.insert(
                session_id,
                SessionState::Config {
                    actor: actor_handle,
                    workspace_path,
                    return_to: None,
                },
            );

            return Ok(());
        };

        // Load workspace mods configuration
        let mods_config = self.load_mods(&workspace_path)?;

        // If no mods config, create one from recommendations and proceed
        let mods_config = match mods_config {
            Some(config) => config,
            None => {
                tracing::debug!("handle_new_session: no workspace mods, applying recommendations");
                let workspace_recs = self.recommendations_for_workspace(&workspace_path);
                let config = WorkspaceModsConfig::from_sources(workspace_recs.mod_sources());

                // Save the new mods config
                if let Err(e) = config.save(&self.config_paths, &workspace_path) {
                    tracing::warn!("Failed to save initial mods config: {}", e);
                }

                config
            }
        };

        tracing::debug!(
            ?agent,
            ?mods_config,
            "handle_new_session: found configuration"
        );

        // Check for recommendation diff on mods
        if let Some(recs) = self.load_recommendations() {
            let workspace_recs = recs.for_workspace(&workspace_path);
            if let Some(diff) = workspace_recs.diff_against(&mods_config) {
                tracing::debug!(?diff, "handle_new_session: diff computed");

                let session_id = SessionId::new(uuid::Uuid::new_v4().to_string());
                request_cx.respond(NewSessionResponse::new(session_id.clone()))?;

                let actor_handle = ConfigModeHandle::spawn_with_recommendations(
                    agent,
                    mods_config,
                    workspace_path.clone(),
                    self.config_paths.clone(),
                    diff,
                    session_id.clone(),
                    config_agent_tx.clone(),
                    cx,
                )?;

                self.sessions.insert(
                    session_id,
                    SessionState::Config {
                        actor: actor_handle,
                        workspace_path,
                        return_to: None,
                    },
                );

                return Ok(());
            }
        }

        tracing::debug!(
            ?agent,
            ?mods_config,
            "handle_new_session: launching new session"
        );

        // No diff changes - proceed directly to uberconductor
        uberconductor
            .new_session(workspace_path, agent, mods_config.mods, request, request_cx)
            .await
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
        // Get the current session state to potentially preserve the conductor and workspace path
        let (return_to, workspace_path) = match self.sessions.get(&session_id) {
            Some(SessionState::Delegating {
                conductor,
                workspace_path,
            }) => (conductor.clone(), workspace_path.clone()),
            Some(SessionState::Config { .. }) | None => {
                // Can't enter config mode while diff is pending
                return request_cx.respond_with_error(sacp::Error::new(
                    -32600,
                    "Cannot only enter config mode when deleating",
                ));
            }
        };

        // Pause the conductor if we have one - it will resume when config mode exits
        let resume_tx = return_to.pause().await?;

        // Load current agent and mods
        let agent = self.load_global_agent()?;
        let mods = self.load_mods(&workspace_path)?;

        // Spawn the config mode actor (it holds resume_tx and drops it on exit)
        let actor_handle = match (agent, mods) {
            // The normal case: both exist, configure them
            (Some(agent), Some(mods)) => ConfigModeHandle::spawn_reconfig(
                agent,
                mods,
                workspace_path.clone(),
                self.config_paths.clone(),
                session_id.clone(),
                config_agent_tx.clone(),
                resume_tx,
                cx,
            )?,
            // If for some reason the configuration has vanished, reinitialize
            _ => {
                let workspace_recs = self.recommendations_for_workspace(&workspace_path);
                ConfigModeHandle::spawn_initial_config(
                    workspace_path.clone(),
                    self.config_paths.clone(),
                    workspace_recs,
                    session_id.clone(),
                    config_agent_tx.clone(),
                    Some(resume_tx),
                    cx,
                )?
            }
        };

        // Transition to config state
        self.sessions.insert(
            session_id,
            SessionState::Config {
                actor: actor_handle,
                workspace_path,
                return_to: Some(return_to),
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
            Some(SessionState::Delegating { conductor, .. }) => {
                // Check if this is the config command
                if Self::is_config_command(&request) {
                    self.enter_config_mode(session_id, request_cx, cx, config_agent_tx)
                        .await
                } else {
                    // Forward to conductor
                    conductor.send_prompt(request, request_cx).await
                }
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
    /// Handle a message for an existing session by forwarding to the appropriate conductor.
    async fn handle_session_message(
        &self,
        session_id: &SessionId,
        message: MessageCx,
    ) -> Result<(), sacp::Error> {
        match self.sessions.get(session_id) {
            Some(SessionState::Delegating { conductor, .. }) => {
                conductor.forward_message(message).await
            }
            Some(SessionState::Config { .. }) => {
                // Config mode and pending diff don't handle arbitrary messages
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
                    self.handle_new_session(request, request_cx, uberconductor, cx, config_agent_tx)
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

#[derive(Debug)]
pub enum ConfigAgentMessage {
    /// Sent when a client sends a message to the agent.
    MessageFromClient(MessageCx),

    /// Sent when a conductor wants to send a message to the client.
    /// ConfigAgent forwards this (and can inject additional messages like session updates).
    MessageToClient(MessageCx),

    /// Sent when a conductor has established a session.
    /// ConfigAgent stores the session mapping, then responds to the client.
    NewSessionCreated {
        response: NewSessionResponse,
        conductor: ConductorHandle,
        workspace_path: PathBuf,
        request_cx: JrRequestCx<NewSessionResponse>,
    },

    /// Output from a config mode actor.
    ConfigModeOutput(SessionId, ConfigModeOutput),
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
