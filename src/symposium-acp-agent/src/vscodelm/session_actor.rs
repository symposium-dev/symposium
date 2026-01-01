//! Session actor for VS Code Language Model Provider
//!
//! Each session actor manages a single conversation with an ACP agent. The actor pattern
//! isolates session state and enables clean cancellation via channel closure.

use elizacp::ElizaAgent;
use sacp::{
    schema::{InitializeRequest, ProtocolVersion, SessionNotification, SessionUpdate, StopReason},
    util::MatchMessage,
    ClientToAgent, Component,
};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use tokio::sync::mpsc;
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
struct SessionMessage {
    /// New messages to process (not the full history, just what's new)
    new_messages: Vec<Message>,
    /// Channel for streaming response parts back
    reply_tx: mpsc::UnboundedSender<ResponsePart>,
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
        let (tx, rx) = mpsc::unbounded_channel();
        let session_id = Uuid::new_v4();
        tracing::info!(%session_id, ?agent_definition, "spawning new session actor");
        cx.spawn(Self::run(rx, agent_definition.clone(), session_id))?;
        Ok(Self {
            tx,
            session_id,
            history: Vec::new(),
            agent_definition,
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
        let (reply_tx, reply_rx) = mpsc::unbounded_channel();

        // Update our history with what we're sending
        self.history.extend(new_messages.clone());

        // Send to the actor (ignore errors - actor may have died)
        let _ = self.tx.send(SessionMessage {
            new_messages,
            reply_tx,
        });

        reply_rx
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

                // Process messages from the handler
                while let Some(msg) = rx.recv().await {
                    let new_message_count = msg.new_messages.len();
                    tracing::debug!(%session_id, new_message_count, "received new messages");

                    // Build prompt from new messages
                    // For now, just concatenate user messages
                    let prompt_text: String = msg
                        .new_messages
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
                    loop {
                        let update = session.read_update().await?;
                        match update {
                            sacp::SessionMessage::SessionMessage(message) => {
                                // Use MatchMessage to extract session notifications
                                let reply_tx = &msg.reply_tx;
                                MatchMessage::new(message)
                                    .if_notification(async |notification: SessionNotification| {
                                        if let SessionUpdate::AgentMessageChunk(chunk) =
                                            notification.update
                                        {
                                            // Convert content block to text
                                            let text = content_block_to_string(&chunk.content);
                                            if !text.is_empty() {
                                                if reply_tx
                                                    .send(ResponsePart::Text { value: text })
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
                                    .otherwise(async |_msg| Ok(()))
                                    .await?;
                            }
                            sacp::SessionMessage::StopReason(stop_reason) => {
                                tracing::debug!(%session_id, ?stop_reason, "agent turn complete");
                                match stop_reason {
                                    StopReason::EndTurn => break,
                                    StopReason::Cancelled => break,
                                    other => {
                                        tracing::warn!(
                                            %session_id,
                                            ?other,
                                            "unexpected stop reason"
                                        );
                                        break;
                                    }
                                }
                            }
                            other => {
                                tracing::trace!(%session_id, ?other, "ignoring session message");
                            }
                        }
                    }

                    tracing::debug!(%session_id, "finished processing request");
                    // reply_tx drops here when msg goes out of scope, signaling completion
                }

                tracing::debug!(%session_id, "session actor shutting down");
                Ok(())
            })
            .await
    }
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
