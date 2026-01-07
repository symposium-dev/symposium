use std::pin::Pin;

use futures::{
    channel::{mpsc, oneshot},
    stream::Peekable,
    StreamExt,
};

use crate::vscodelm::{session_actor::ModelRequest, ContentPart, Message};

pub struct RequestResponse {
    /// Response parts sent so far
    parts: Vec<ContentPart>,

    /// Channel for streaming response parts back.
    /// Drop to indicate that we are waiting for a new request before continuing.
    prompt_tx: mpsc::UnboundedSender<ContentPart>,

    /// Receiving `()` on this channel indicates cancellation.
    cancel_rx: oneshot::Receiver<()>,
}

impl RequestResponse {
    pub fn new(
        prompt_tx: mpsc::UnboundedSender<ContentPart>,
        cancel_rx: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            parts: Default::default(),
            prompt_tx,
            cancel_rx,
        }
    }

    /// Internal method: send a part back to vscode, recording
    /// it in our internal vector for future reference.
    fn send_any_part(&mut self, part: ContentPart) -> Result<(), Canceled> {
        tracing::debug!(?part, "send_any_part");

        self.parts.push(part.clone());
        self.prompt_tx.unbounded_send(part).map_err(|_| Canceled)
    }

    /// Send the text part back to VSCode
    pub fn send_text_part(&mut self, text: impl ToString) -> Result<(), Canceled> {
        self.send_any_part(ContentPart::Text {
            value: text.to_string(),
        })
    }

    /// Send a tool use part back to VSCode and return with the response we get.
    ///
    /// Internally, this will end the VSCode prompt and await a new message.
    /// The new message is expected to contain all the parts that we have sent so far
    /// (including the tool-use) *plus* a new message with the result of the tool call.
    pub async fn send_tool_use(
        mut self,
        call_id: String,
        name: String,
        input: serde_json::Value,
        mut actor_rx: Pin<&mut Peekable<mpsc::UnboundedReceiver<ModelRequest>>>,
    ) -> Result<SendToolUseResult, Canceled> {
        tracing::debug!(?call_id, ?name, ?input, "send_tool_use");

        // Start by sending the tool-call (which will get recorded in self.parts).
        self.send_any_part(ContentPart::ToolCall {
            tool_call_id: call_id.clone(),
            tool_name: name,
            parameters: input,
        })?;

        let Self {
            parts,
            prompt_tx,
            cancel_rx,
        } = self;

        // Drop the `prompt_tx` to indicate that we have completed.
        drop(prompt_tx);
        drop(cancel_rx);

        // Wait for VSCode to respond. If the stream ends, just cancel.
        let Some(peek_request) = actor_rx.as_mut().peek().await else {
            return Err(Canceled);
        };
        tracing::debug!(?peek_request, "next request received");

        // Validate the response and extract the tool result
        let tool_result =
            validate_tool_response(&peek_request.new_messages, &parts, &call_id).ok_or(Canceled)?;

        // Consume the request (we only peeked before)
        let request = actor_rx.next().await.ok_or(Canceled)?;

        // Build a new RequestResponse to continue streaming
        let new_response = RequestResponse::new(request.prompt_tx, request.cancel_rx);

        Ok(SendToolUseResult {
            request_response: new_response,
            tool_result,
        })
    }
}

/// Validates that the new messages match the expected tool call flow.
///
/// We expect exactly two new messages:
/// 1. **An Assistant message** containing:
///    - Any text we streamed before the tool call
///    - The `LanguageModelToolCallPart` we emitted
/// 2. **A User message** containing:
///    - `LanguageModelToolResultPart` with the matching `callId` and result content
///
/// Returns the tool result value on success, or `None` if validation fails.
fn validate_tool_response(
    new_messages: &[Message],
    parts: &[ContentPart],
    call_id: &str,
) -> Option<serde_json::Value> {
    let [assistant_msg, user_msg] = new_messages else {
        tracing::debug!(
            message_count = new_messages.len(),
            "expected exactly 2 messages"
        );
        return None;
    };

    // Validate assistant message: role and content must match what we sent
    if assistant_msg.role != crate::vscodelm::ROLE_ASSISTANT {
        tracing::debug!("expected assistant message, got {:?}", assistant_msg.role);
        return None;
    }

    if assistant_msg.content != *parts {
        tracing::debug!(
            ?assistant_msg.content,
            ?parts,
            "assistant message content doesn't match sent parts"
        );
        return None;
    }

    // Validate user message: must be user role with matching tool result
    if user_msg.role != crate::vscodelm::ROLE_USER {
        tracing::debug!("expected user message, got {:?}", user_msg.role);
        return None;
    }

    // Find the tool result with matching call_id
    let tool_result = user_msg.content.iter().find_map(|part| match part {
        ContentPart::ToolResult {
            tool_call_id,
            result,
        } if tool_call_id == call_id => Some(result.clone()),
        _ => None,
    })?;

    Some(tool_result)
}

pub struct SendToolUseResult {
    pub request_response: RequestResponse,
    pub tool_result: serde_json::Value,
}

pub struct Canceled;
