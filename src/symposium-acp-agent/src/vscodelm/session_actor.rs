//! Session actor for VS Code Language Model Provider
//!
//! Each session actor manages a single conversation with an ACP agent. The actor pattern
//! isolates session state and enables clean cancellation via channel closure.

use elizacp::ElizaAgent;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use sacp::{
    schema::{
        InitializeRequest, ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    },
    ClientToAgent, Component, MessageCx,
};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use uuid::Uuid;

use super::{LmBackendToVsCode, Message, ResponsePart};

/// Defines which agent backend to use for a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDefinition {
    /// Use the in-process Eliza chatbot (for testing)
    Eliza {
        #[serde(default)]
        deterministic: bool,
    },
    /// Spawn an external ACP agent process
    McpServer(sacp::schema::McpServer),
}

/// Message sent to the session actor
enum SessionMessage {
    /// A new prompt to process
    Prompt {
        /// New messages to process (not the full history, just what's new)
        new_messages: Vec<Message>,
        /// Channel for streaming response parts back
        reply_tx: mpsc::UnboundedSender<ResponsePart>,
    },
    /// Permission decision from VS Code (tool was approved or rejected)
    PermissionDecision {
        /// Whether the permission was approved
        approved: bool,
        /// Channel for streaming continued response parts back
        reply_tx: mpsc::UnboundedSender<ResponsePart>,
    },
}

/// Information about a pending permission request
struct PendingPermission {
    /// The tool call ID we emitted to VS Code
    tool_call_id: String,
    /// Channel to send the decision back to the agent loop
    decision_tx: oneshot::Sender<bool>,
}

/// State of the session from the handler's perspective
#[derive(Debug)]
pub enum ActorState {
    /// Ready for a new prompt
    Idle,
    /// Awaiting permission decision from VS Code
    AwaitingPermission {
        /// The tool call ID we're waiting for
        tool_call_id: String,
    },
}

/// Handle for communicating with a session actor.
///
/// This follows the Tokio actor pattern: the handle owns a sender channel and provides
/// methods for interacting with the actor. The actor itself runs in a spawned task.
pub struct SessionActor {
    tx: mpsc::UnboundedSender<SessionMessage>,
    /// Unique identifier for this session (for logging)
    session_id: Uuid,
    /// The message history this session has processed
    history: Vec<Message>,
    /// The agent definition (stored for future prefix matching)
    #[allow(dead_code)]
    agent_definition: AgentDefinition,
    /// Current state of the actor
    state: ActorState,
}

impl SessionActor {
    /// Spawn a new session actor.
    ///
    /// Creates the actor's mailbox and spawns the run loop. Returns a handle
    /// for sending messages to the actor.
    pub fn spawn(
        cx: &sacp::JrConnectionCx<LmBackendToVsCode>,
        agent_definition: AgentDefinition,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::unbounded();
        let session_id = Uuid::new_v4();
        tracing::info!(%session_id, ?agent_definition, "spawning new session actor");
        cx.spawn(Self::run(rx, agent_definition.clone(), session_id))?;
        Ok(Self {
            tx,
            session_id,
            history: Vec::new(),
            agent_definition,
            state: ActorState::Idle,
        })
    }

    /// Returns the session ID (for logging).
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Send new content to the actor, returns a receiver for streaming response.
    ///
    /// The caller should stream from the returned receiver until it closes,
    /// which signals that the actor has finished processing.
    ///
    /// To cancel the request, simply drop the receiver - the actor will see
    /// send failures and stop processing.
    pub fn send_prompt(
        &mut self,
        new_messages: Vec<Message>,
    ) -> mpsc::UnboundedReceiver<ResponsePart> {
        let (reply_tx, reply_rx) = mpsc::unbounded();

        // Update our history with what we're sending
        self.history.extend(new_messages.clone());

        // Send to the actor (ignore errors - actor may have died)
        let _ = self.tx.unbounded_send(SessionMessage::Prompt {
            new_messages,
            reply_tx,
        });

        reply_rx
    }

    /// Send a permission decision to the actor, returns a receiver for streaming response.
    pub fn send_permission_decision(
        &mut self,
        approved: bool,
    ) -> mpsc::UnboundedReceiver<ResponsePart> {
        // Reset state to idle
        self.state = ActorState::Idle;

        let (reply_tx, reply_rx) = mpsc::unbounded();

        let _ = self
            .tx
            .unbounded_send(SessionMessage::PermissionDecision { approved, reply_tx });

        reply_rx
    }

    /// Get the current state of the actor.
    pub fn state(&self) -> &ActorState {
        &self.state
    }

    /// Set the actor state to awaiting permission.
    pub fn set_awaiting_permission(&mut self, tool_call_id: String) {
        self.state = ActorState::AwaitingPermission { tool_call_id };
    }

    /// Check if incoming messages extend our history.
    ///
    /// Returns the number of matching prefix messages, or None if the incoming
    /// messages don't start with our history.
    pub fn prefix_match_len(&self, messages: &[Message]) -> Option<usize> {
        if messages.len() < self.history.len() {
            return None;
        }
        if self
            .history
            .iter()
            .zip(messages.iter())
            .all(|(a, b)| a == b)
        {
            Some(self.history.len())
        } else {
            None
        }
    }

    /// The actor's main run loop.
    async fn run(
        rx: mpsc::UnboundedReceiver<SessionMessage>,
        agent_definition: AgentDefinition,
        session_id: Uuid,
    ) -> Result<(), sacp::Error> {
        tracing::debug!(%session_id, "session actor starting");

        match agent_definition {
            AgentDefinition::Eliza { deterministic } => {
                let agent = ElizaAgent::new(deterministic);
                Self::run_with_agent(rx, agent, session_id).await
            }
            AgentDefinition::McpServer(config) => {
                let agent = AcpAgent::new(config);
                Self::run_with_agent(rx, agent, session_id).await
            }
        }
    }

    /// Run the session with a specific agent component.
    async fn run_with_agent(
        mut rx: mpsc::UnboundedReceiver<SessionMessage>,
        agent: impl Component<sacp::link::AgentToClient>,
        session_id: Uuid,
    ) -> Result<(), sacp::Error> {
        ClientToAgent::builder()
            .connect_to(agent)?
            .run_until(async |cx| {
                tracing::debug!(%session_id, "connected to agent, initializing");

                // Initialize the agent
                let _init_response = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;

                tracing::debug!(%session_id, "agent initialized, creating session");

                // Create a session
                let mut session = cx
                    .build_session(PathBuf::from("."))
                    .block_task()
                    .start_session()
                    .await?;

                tracing::debug!(%session_id, "session created, waiting for messages");

                // Track pending permission request (if any)
                let mut pending_permission: Option<PendingPermission> = None;

                // Process messages from the handler
                while let Some(msg) = rx.next().await {
                    match msg {
                        SessionMessage::Prompt {
                            new_messages,
                            reply_tx,
                        } => {
                            let new_message_count = new_messages.len();
                            tracing::debug!(%session_id, new_message_count, "received new messages");

                            // Build prompt from new messages
                            // For now, just concatenate user messages
                            let prompt_text: String = new_messages
                                .iter()
                                .filter(|m| m.role == "user")
                                .map(|m| m.text())
                                .collect::<Vec<_>>()
                                .join("\n");

                            if prompt_text.is_empty() {
                                tracing::debug!(%session_id, "no user messages, skipping");
                                continue;
                            }

                            tracing::debug!(%session_id, %prompt_text, "sending prompt to agent");
                            session.send_prompt(&prompt_text)?;

                            // Read updates and stream back
                            // This may exit early if we hit a permission request
                            let result = loop {
                                let update = session.read_update().await?;
                                match update {
                                    sacp::SessionMessage::SessionMessage(message) => {
                                        if let Some(loop_result) = Self::process_session_message(
                                            message,
                                            &reply_tx,
                                            &mut pending_permission,
                                            session_id,
                                        )
                                        .await?
                                        {
                                            break loop_result;
                                        }
                                    }
                                    sacp::SessionMessage::StopReason(stop_reason) => {
                                        tracing::debug!(
                                            %session_id,
                                            ?stop_reason,
                                            "agent turn complete"
                                        );
                                        break UpdateLoopResult::TurnComplete;
                                    }
                                    other => {
                                        tracing::trace!(
                                            %session_id,
                                            ?other,
                                            "ignoring session message"
                                        );
                                    }
                                }
                            };

                            match result {
                                UpdateLoopResult::TurnComplete => {
                                    tracing::debug!(%session_id, "finished processing request");
                                }
                                UpdateLoopResult::AwaitingPermission { tool_call_id } => {
                                    tracing::debug!(
                                        %session_id,
                                        %tool_call_id,
                                        "awaiting permission decision"
                                    );
                                    // reply_tx will be dropped, ending the VS Code stream
                                }
                            }
                        }
                        SessionMessage::PermissionDecision { approved, reply_tx } => {
                            tracing::debug!(%session_id, approved, "received permission decision");

                            if let Some(pending) = pending_permission.take() {
                                // Send decision through the oneshot to unblock the spawned task
                                let _ = pending.decision_tx.send(approved);

                                // If approved, continue reading updates and streaming them
                                if approved {
                                    tracing::debug!(%session_id, "permission approved, continuing to stream");
                                    // Continue the update loop with the new reply channel
                                    let result = loop {
                                        let update = session.read_update().await?;
                                        match update {
                                            sacp::SessionMessage::SessionMessage(message) => {
                                                if let Some(loop_result) =
                                                    Self::process_session_message(
                                                        message,
                                                        &reply_tx,
                                                        &mut pending_permission,
                                                        session_id,
                                                    )
                                                    .await?
                                                {
                                                    break loop_result;
                                                }
                                            }
                                            sacp::SessionMessage::StopReason(stop_reason) => {
                                                tracing::debug!(
                                                    %session_id,
                                                    ?stop_reason,
                                                    "agent turn complete after permission"
                                                );
                                                break UpdateLoopResult::TurnComplete;
                                            }
                                            other => {
                                                tracing::trace!(
                                                    %session_id,
                                                    ?other,
                                                    "ignoring session message"
                                                );
                                            }
                                        }
                                    };

                                    match result {
                                        UpdateLoopResult::TurnComplete => {
                                            tracing::debug!(
                                                %session_id,
                                                "finished processing after permission"
                                            );
                                        }
                                        UpdateLoopResult::AwaitingPermission { tool_call_id } => {
                                            tracing::debug!(
                                                %session_id,
                                                %tool_call_id,
                                                "awaiting another permission decision"
                                            );
                                        }
                                    }
                                } else {
                                    tracing::debug!(%session_id, "permission rejected");
                                    // reply_tx is dropped, ending the stream
                                }
                            } else {
                                tracing::warn!(
                                    %session_id,
                                    "received permission decision but no pending request"
                                );
                            }
                        }
                    }
                }

                tracing::debug!(%session_id, "session actor shutting down");
                Ok(())
            })
            .await
    }

    /// Process a single session message from the agent.
    ///
    /// Returns `Some(result)` if we should exit the update loop, `None` to continue.
    async fn process_session_message(
        message: MessageCx,
        reply_tx: &mpsc::UnboundedSender<ResponsePart>,
        pending_permission: &mut Option<PendingPermission>,
        session_id: Uuid,
    ) -> Result<Option<UpdateLoopResult>, sacp::Error> {
        use sacp::util::MatchMessage;

        let mut loop_result: Option<UpdateLoopResult> = None;

        MatchMessage::new(message)
            .if_notification(async |notif: SessionNotification| {
                if let SessionUpdate::AgentMessageChunk(chunk) = notif.update {
                    let text = content_block_to_string(&chunk.content);
                    if !text.is_empty() {
                        if reply_tx
                            .unbounded_send(ResponsePart::Text { value: text })
                            .is_err()
                        {
                            tracing::debug!(
                                %session_id,
                                "reply channel closed, request cancelled"
                            );
                        }
                    }
                }
                Ok(())
            })
            .await
            .if_request(async |perm_request: RequestPermissionRequest, request_cx| {
                tracing::debug!(
                    %session_id,
                    ?perm_request,
                    "received permission request from agent"
                );

                // Extract tool call info
                let tool_call_id = perm_request.tool_call.tool_call_id.0.to_string();
                let title = perm_request
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "Agent action".to_string());
                let kind = perm_request
                    .tool_call
                    .fields
                    .kind
                    .as_ref()
                    .map(|k| format!("{:?}", k))
                    .unwrap_or_default();

                // Emit tool call to VS Code
                let _ = reply_tx.unbounded_send(ResponsePart::ToolCall {
                    call_id: tool_call_id.clone(),
                    name: "symposium-agent-action".to_string(),
                    input: serde_json::json!({
                        "toolCallId": tool_call_id,
                        "title": title,
                        "kind": kind,
                    }),
                });

                // Create channel for permission decision
                let (decision_tx, decision_rx) = oneshot::channel();

                // Store pending permission
                *pending_permission = Some(PendingPermission {
                    tool_call_id: tool_call_id.clone(),
                    decision_tx,
                });

                // Get the first option_id to use for approval
                let first_option_id = perm_request.options.first().map(|o| o.option_id.clone());

                // Spawn task to wait for decision and respond to the agent
                tokio::spawn(async move {
                    let response = match decision_rx.await {
                        Ok(true) => {
                            // Approved - respond with first option
                            if let Some(option_id) = first_option_id {
                                RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                                    SelectedPermissionOutcome::new(option_id),
                                ))
                            } else {
                                // No options, respond with cancelled
                                RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
                            }
                        }
                        Ok(false) | Err(_) => {
                            // Rejected or channel dropped - respond with cancelled
                            RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
                        }
                    };
                    let _ = request_cx.respond(response);
                });

                // Send marker to signal the handler that we're awaiting permission
                let _ = reply_tx.unbounded_send(ResponsePart::AwaitingPermission {
                    tool_call_id: tool_call_id.clone(),
                });

                loop_result = Some(UpdateLoopResult::AwaitingPermission { tool_call_id });
                Ok(())
            })
            .await
            .otherwise(async |message| {
                match message {
                    MessageCx::Request(request, _) => {
                        tracing::warn!(
                            %session_id,
                            method = request.method(),
                            "unknown request from agent"
                        );
                    }
                    MessageCx::Notification(notif) => {
                        tracing::trace!(
                            %session_id,
                            method = notif.method(),
                            "ignoring unhandled notification"
                        );
                    }
                }
                Ok(())
            })
            .await?;

        Ok(loop_result)
    }
}

/// Result of processing agent updates
enum UpdateLoopResult {
    /// Turn completed normally
    TurnComplete,
    /// Awaiting permission decision from VS Code
    AwaitingPermission { tool_call_id: String },
}

/// Convert a content block to a string representation
fn content_block_to_string(block: &sacp::schema::ContentBlock) -> String {
    use sacp::schema::{ContentBlock, EmbeddedResourceResource};
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(img) => format!("[Image: {}]", img.mime_type),
        ContentBlock::Audio(audio) => format!("[Audio: {}]", audio.mime_type),
        ContentBlock::ResourceLink(link) => link.uri.clone(),
        ContentBlock::Resource(resource) => match &resource.resource {
            EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
            EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
            _ => "[Unknown resource type]".to_string(),
        },
        _ => "[Unknown content type]".to_string(),
    }
}
