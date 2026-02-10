//! History Actor for VS Code Language Model Provider
//!
//! The HistoryActor owns all session state and handles history matching.
//! It receives messages from both VS Code (via the JrConnectionCx handler)
//! and from SessionActors (outgoing parts). This centralizes all mutable
//! state in one actor with proper &mut access.

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use uuid::Uuid;

use super::session_actor::{AgentDefinition, SessionActor};
use super::{
    ContentPart, Message, ProvideResponseRequest, ProvideResponseResponse, ROLE_ASSISTANT,
    ResponseCompleteNotification, ResponsePartNotification, SYMPOSIUM_AGENT_ACTION,
    normalize_messages,
};
use sacp::JrConnectionCx;

use super::LmBackendToVsCode;

// ============================================================================
// Messages to HistoryActor
// ============================================================================

/// Messages that can be sent to the HistoryActor's mailbox.
pub enum HistoryActorMessage {
    /// A request from VS Code
    FromVsCode {
        request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    },
    /// A cancel notification from VS Code
    CancelFromVsCode { request_id: serde_json::Value },
    /// A message from a SessionActor
    FromSession {
        session_id: Uuid,
        message: SessionToHistoryMessage,
    },
}

/// Messages from SessionActor to HistoryActor
pub enum SessionToHistoryMessage {
    /// A response part to forward to VS Code
    Part(ContentPart),
    /// The response is complete
    Complete,
    /// The session encountered an error
    Error(String),
}

// ============================================================================
// Handle for sending to HistoryActor
// ============================================================================

/// Handle for sending messages to the HistoryActor.
/// SessionActors hold this to send parts back.
#[derive(Clone)]
pub struct HistoryActorHandle {
    tx: mpsc::UnboundedSender<HistoryActorMessage>,
}

impl HistoryActorHandle {
    /// Send a message from a session to the history actor.
    pub fn send_from_session(
        &self,
        session_id: Uuid,
        message: SessionToHistoryMessage,
    ) -> Result<(), sacp::Error> {
        self.tx
            .unbounded_send(HistoryActorMessage::FromSession {
                session_id,
                message,
            })
            .map_err(|_| sacp::util::internal_error("no history actor"))
    }

    /// Send a VS Code request to the history actor.
    pub fn send_from_vscode(
        &self,
        request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    ) -> Result<(), sacp::Error> {
        self.tx
            .unbounded_send(HistoryActorMessage::FromVsCode {
                request,
                request_id,
                request_cx,
            })
            .map_err(|_| sacp::util::internal_error("no history actor"))
    }

    /// Send a cancel notification from VS Code.
    pub fn send_cancel_from_vscode(
        &self,
        request_id: serde_json::Value,
    ) -> Result<(), sacp::Error> {
        self.tx
            .unbounded_send(HistoryActorMessage::CancelFromVsCode { request_id })
            .map_err(|_| sacp::util::internal_error("no history actor"))
    }
}

// ============================================================================
// Session Data (history tracking per session)
// ============================================================================

/// Data for a single session, owned by HistoryActor.
struct SessionData {
    /// The session actor handle
    actor: SessionActor,
    /// The agent definition (for matching)
    agent_definition: AgentDefinition,
    /// Committed messages: complete history VS Code has acknowledged
    committed: Vec<Message>,
    /// Provisional messages: what we've received plus assistant response being built
    provisional_messages: Vec<Message>,
    /// Current streaming state
    streaming: Option<StreamingState>,
    /// Whether the internal tool (symposium-agent-action) is available.
    /// If false, all permission requests should be auto-denied.
    has_internal_tool: bool,
}

/// State when actively streaming a response
struct StreamingState {
    /// The JSON-RPC request ID of the in-flight request
    request_id: serde_json::Value,
    /// The request context for responding when done
    request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    /// Channel to signal cancellation
    ///
    /// We never actually send a signal on this channel, we just
    /// drop it once we stop streaming.
    #[expect(dead_code)]
    cancel_tx: oneshot::Sender<()>,
}

/// Result of matching incoming messages against session history.
struct HistoryMatch {
    /// New messages to process (after matched prefix)
    new_messages: Vec<Message>,
    /// Whether the provisional work was canceled
    canceled: bool,
}

impl SessionData {
    fn new(
        actor: SessionActor,
        agent_definition: AgentDefinition,
        has_internal_tool: bool,
    ) -> Self {
        Self {
            actor,
            agent_definition,
            committed: Vec::new(),
            provisional_messages: Vec::new(),
            streaming: None,
            has_internal_tool,
        }
    }

    /// Check if incoming messages match our expected history and return match info.
    fn match_history(&self, incoming: &[Message]) -> Option<HistoryMatch> {
        let committed_len = self.committed.len();

        tracing::trace!(
            ?incoming,
            ?self.committed,
            ?self.provisional_messages,
            "match_history"
        );

        // Incoming must at least start with committed
        if incoming.len() < committed_len {
            tracing::trace!(
                incoming_len = incoming.len(),
                committed_len,
                "match_history: incoming shorter than committed"
            );
            return None;
        }
        if &incoming[..committed_len] != self.committed.as_slice() {
            tracing::trace!(committed_len, "match_history: committed prefix mismatch");
            return None;
        }

        let after_committed = &incoming[committed_len..];

        // Check if the new messages have the provisional messages as a prefix
        if !after_committed.starts_with(&self.provisional_messages) {
            // They do not. This must be a cancellation of the provisional content.
            tracing::debug!(
                after_committed_len = after_committed.len(),
                provisional_len = self.provisional_messages.len(),
                "match_history: provisional mismatch, marking as canceled"
            );
            // Log the first differing message for debugging
            for (i, (incoming_msg, provisional_msg)) in after_committed
                .iter()
                .zip(&self.provisional_messages)
                .enumerate()
            {
                if incoming_msg != provisional_msg {
                    tracing::debug!(
                        index = i,
                        ?incoming_msg,
                        ?provisional_msg,
                        "match_history: first mismatch"
                    );
                    break;
                }
            }
            return Some(HistoryMatch {
                new_messages: after_committed.to_vec(),
                canceled: true,
            });
        }

        Some(HistoryMatch {
            new_messages: after_committed[self.provisional_messages.len()..].to_vec(),
            canceled: false,
        })
    }

    /// Record that we're sending a response part.
    fn record_part(&mut self, part: ContentPart) {
        match self.provisional_messages.last_mut() {
            Some(msg) if msg.role == ROLE_ASSISTANT => {
                msg.content.push(part);
            }
            _ => {
                self.provisional_messages.push(Message {
                    role: ROLE_ASSISTANT.to_string(),
                    content: vec![part],
                });
            }
        }
    }

    /// Commit the provisional exchange.
    fn commit_provisional(&mut self) {
        self.committed.append(&mut self.provisional_messages);
    }

    /// Discard provisional.
    fn discard_provisional(&mut self) {
        self.provisional_messages.clear();
    }

    /// Start a new provisional exchange.
    fn start_provisional(&mut self, messages: Vec<Message>) {
        assert!(self.provisional_messages.is_empty());
        self.provisional_messages.extend(messages);
    }
}

// ============================================================================
// HistoryActor
// ============================================================================

/// The HistoryActor owns all session state and handles history matching.
pub struct HistoryActor {
    /// Mailbox receiver
    rx: mpsc::UnboundedReceiver<HistoryActorMessage>,
    /// Handle for creating new session actors
    handle: HistoryActorHandle,
    /// Connection to VS Code for sending notifications
    cx: JrConnectionCx<LmBackendToVsCode>,
    /// All sessions
    sessions: Vec<SessionData>,
}

impl HistoryActor {
    /// Create a new HistoryActor and return a handle to it.
    pub fn new(cx: &JrConnectionCx<LmBackendToVsCode>) -> Result<HistoryActorHandle, sacp::Error> {
        let (tx, rx) = mpsc::unbounded();
        let handle = HistoryActorHandle { tx };
        let actor = Self {
            rx,
            handle: handle.clone(),
            cx: cx.clone(),
            sessions: Vec::new(),
        };
        cx.spawn(async move { actor.run().await })?;
        Ok(handle)
    }

    /// Run the actor's main loop.
    pub async fn run(mut self) -> Result<(), sacp::Error> {
        while let Some(msg) = self.rx.next().await {
            match msg {
                HistoryActorMessage::FromVsCode {
                    request,
                    request_id,
                    request_cx,
                } => {
                    self.handle_vscode_request(request, request_id, request_cx)?;
                }
                HistoryActorMessage::CancelFromVsCode { request_id } => {
                    self.handle_vscode_cancel(request_id);
                }
                HistoryActorMessage::FromSession {
                    session_id,
                    message,
                } => {
                    self.handle_session_message(session_id, message)?;
                }
            }
        }
        Ok(())
    }

    /// Handle a request from VS Code.
    fn handle_vscode_request(
        &mut self,
        mut request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    ) -> Result<(), sacp::Error> {
        tracing::debug!(
            message_count = request.messages.len(),
            "received VS Code request"
        );

        // Normalize incoming messages to coalesce consecutive text parts.
        // This ensures consistent comparison with our provisional history.
        normalize_messages(&mut request.messages);

        // Find session with best history match (must also match agent)
        let best_match = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.agent_definition == request.agent)
            .filter_map(|(i, s)| s.match_history(&request.messages).map(|m| (i, m)))
            .max_by_key(|(_, m)| !m.canceled); // prefer non-canceled matches

        // Check if the internal tool is available in the request options
        let has_internal_tool = request
            .options
            .tools
            .iter()
            .any(|t| t.name == SYMPOSIUM_AGENT_ACTION);

        let (session_idx, history_match) = if let Some((idx, history_match)) = best_match {
            tracing::debug!(
                session_id = %self.sessions[idx].actor.session_id(),
                canceled = history_match.canceled,
                new_message_count = history_match.new_messages.len(),
                has_internal_tool,
                "matched existing session"
            );
            // Update the tool availability (it can change between requests)
            self.sessions[idx].has_internal_tool = has_internal_tool;
            (idx, history_match)
        } else {
            // No matching session - create a new one
            let actor = SessionActor::spawn(self.handle.clone(), request.agent.clone())?;
            tracing::debug!(
                session_id = %actor.session_id(),
                has_internal_tool,
                "created new session"
            );
            self.sessions.push(SessionData::new(
                actor,
                request.agent.clone(),
                has_internal_tool,
            ));
            let history_match = HistoryMatch {
                new_messages: request.messages.clone(),
                canceled: false,
            };
            (self.sessions.len() - 1, history_match)
        };

        let session_data = &mut self.sessions[session_idx];

        // Handle cancellation if needed
        if history_match.canceled {
            session_data.discard_provisional();
        } else {
            // Commit any previous provisional (new messages confirm it was accepted)
            session_data.commit_provisional();
        }

        // Start new provisional with the new messages
        session_data.start_provisional(history_match.new_messages.clone());

        // Create cancellation
        let (cancel_tx, cancel_rx) = oneshot::channel();

        // Store streaming state
        session_data.streaming = Some(StreamingState {
            request_id,
            request_cx,
            cancel_tx,
        });

        // Extract VS Code tools (excluding our internal tool)
        let vscode_tools: Vec<_> = request
            .options
            .tools
            .into_iter()
            .filter(|t| t.name != SYMPOSIUM_AGENT_ACTION)
            .collect();

        // Send to session actor
        session_data.actor.send_messages(
            history_match.new_messages,
            history_match.canceled,
            cancel_rx,
            session_data.has_internal_tool,
            vscode_tools,
        );

        Ok(())
    }

    /// Handle a cancel notification from VS Code.
    fn handle_vscode_cancel(&mut self, request_id: serde_json::Value) {
        tracing::debug!(?request_id, "HistoryActor: received cancel");

        // Find and cancel the session streaming this request
        if let Some(session_data) = self
            .sessions
            .iter_mut()
            .find(|s| matches!(&s.streaming, Some(st) if st.request_id == request_id))
        {
            // Dropping this will drop the oneshot-sender which
            // effectively sends a cancel message.
            session_data.streaming = None;
            tracing::debug!(
                session_id = %session_data.actor.session_id(),
                "cancelled streaming response"
            );
        } else {
            tracing::warn!(?request_id, "cancel for unknown request");
        }
    }

    /// Handle a message from a SessionActor.
    fn handle_session_message(
        &mut self,
        session_id: Uuid,
        message: SessionToHistoryMessage,
    ) -> Result<(), sacp::Error> {
        let Some(session_data) = self
            .sessions
            .iter_mut()
            .find(|s| s.actor.session_id() == session_id)
        else {
            tracing::warn!(%session_id, "message from unknown session");
            return Ok(());
        };

        // Get the request_id first (before mutable borrows)
        let Some(request_id) = session_data
            .streaming
            .as_ref()
            .map(|s| s.request_id.clone())
        else {
            tracing::warn!(%session_id, "message but not streaming");
            return Ok(());
        };

        match message {
            SessionToHistoryMessage::Part(part) => {
                // Record the part in provisional history
                session_data.record_part(part.clone());

                // Forward to VS Code
                self.cx
                    .send_notification(ResponsePartNotification { request_id, part })?;
            }
            SessionToHistoryMessage::Complete => {
                // Normalize provisional messages before completion.
                // This ensures history matching works correctly on subsequent requests.
                normalize_messages(&mut session_data.provisional_messages);

                // Send completion notification
                self.cx
                    .send_notification(ResponseCompleteNotification { request_id })?;

                // Respond to the request
                let streaming = session_data.streaming.take().unwrap();
                streaming.request_cx.respond(ProvideResponseResponse {})?;
            }
            SessionToHistoryMessage::Error(err) => {
                tracing::error!(%session_id, %err, "session error");
                // Take streaming and respond with error
                if let Some(streaming) = session_data.streaming.take() {
                    streaming
                        .request_cx
                        .respond_with_error(sacp::Error::new(-32000, err))?;
                }
            }
        }

        Ok(())
    }
}
