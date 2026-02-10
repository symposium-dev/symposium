//! VS Code Language Model Provider backend
//!
//! This module implements the Rust backend for the VS Code `LanguageModelChatProvider` API.
//! It uses sacp's JSON-RPC infrastructure for communication with the TypeScript extension.

mod history_actor;
pub mod session_actor;
#[cfg(test)]
mod tests;
mod vscode_tools_mcp;

use anyhow::Result;
use history_actor::{HistoryActor, HistoryActorHandle};
use sacp::{
    Component, Handled, JrConnectionCx, JrLink, JrMessageHandler, JrNotification, JrPeer,
    JrRequest, JrResponsePayload, MessageCx, link::RemoteStyle, util::MatchMessage,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Name of the special tool we inject into vscode for requesting permission
const SYMPOSIUM_AGENT_ACTION: &str = "symposium-agent-action";

/// Role constants for message matching
pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";

// ============================================================================
// Peers
// ============================================================================

/// Peer representing the VS Code extension (TypeScript side).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VsCodePeer;

impl JrPeer for VsCodePeer {}

/// Peer representing the LM backend (Rust side).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LmBackendPeer;

impl JrPeer for LmBackendPeer {}

// ============================================================================
// Links
// ============================================================================

/// Link from the LM backend's perspective (talking to VS Code).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LmBackendToVsCode;

impl JrLink for LmBackendToVsCode {
    type ConnectsTo = VsCodeToLmBackend;
    type State = ();
}

impl sacp::HasDefaultPeer for LmBackendToVsCode {
    type DefaultPeer = VsCodePeer;
}

impl sacp::HasPeer<VsCodePeer> for LmBackendToVsCode {
    fn remote_style(_peer: VsCodePeer) -> RemoteStyle {
        RemoteStyle::Counterpart
    }
}

/// Link from VS Code's perspective (talking to the LM backend).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VsCodeToLmBackend;

impl JrLink for VsCodeToLmBackend {
    type ConnectsTo = LmBackendToVsCode;
    type State = ();
}

impl sacp::HasDefaultPeer for VsCodeToLmBackend {
    type DefaultPeer = LmBackendPeer;
}

impl sacp::HasPeer<LmBackendPeer> for VsCodeToLmBackend {
    fn remote_style(_peer: LmBackendPeer) -> RemoteStyle {
        RemoteStyle::Counterpart
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// Message content part
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        value: String,
    },
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        parameters: serde_json::Value,
    },
    ToolResult {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        result: serde_json::Value,
    },
}

/// A chat message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
}

impl Message {
    /// Extract text content from the message
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { value } => Some(value.as_str()),
                ContentPart::ToolCall { .. } | ContentPart::ToolResult { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Check if the message contains a tool result for the given tool call ID
    pub fn has_tool_result(&self, tool_call_id: &str) -> bool {
        self.content.iter().any(|part| {
            matches!(part, ContentPart::ToolResult { tool_call_id: id, .. } if id == tool_call_id)
        })
    }

    /// Check if the message contains ONLY a tool result for the given tool call ID and nothing else
    pub fn has_just_tool_result(&self, tool_call_id: &str) -> bool {
        self.content.len() == 1 && self.has_tool_result(tool_call_id)
    }

    /// Normalize the message by coalescing consecutive Text parts.
    pub fn normalize(&mut self) {
        let mut normalized = Vec::with_capacity(self.content.len());
        for part in self.content.drain(..) {
            if let ContentPart::Text { value: new_text } = &part {
                if let Some(ContentPart::Text { value: existing }) = normalized.last_mut() {
                    existing.push_str(new_text);
                    continue;
                }
            }
            normalized.push(part);
        }
        self.content = normalized;
    }
}

/// Normalize a vector of messages in place.
pub fn normalize_messages(messages: &mut Vec<Message>) {
    for msg in messages.iter_mut() {
        msg.normalize();
    }
}

// ============================================================================
// Request Options Types
// ============================================================================

/// Tool definition passed in request options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Tool mode for chat requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    #[default]
    Auto,
    Required,
}

/// Options for chat requests from VS Code
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequestOptions {
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub tool_mode: Option<ToolMode>,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub family: String,
    pub version: String,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(default)]
    pub tool_calling: bool,
}

// ----------------------------------------------------------------------------
// lm/provideLanguageModelChatInformation
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideLanguageModelChatInformation", response = ProvideInfoResponse)]
pub struct ProvideInfoRequest {
    #[serde(default)]
    pub silent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideInfoResponse {
    pub models: Vec<ModelInfo>,
}

// ----------------------------------------------------------------------------
// lm/provideLanguageModelChatResponse
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideLanguageModelChatResponse", response = ProvideResponseResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProvideResponseRequest {
    pub model_id: String,
    pub messages: Vec<Message>,
    pub agent: session_actor::AgentDefinition,
    #[serde(default)]
    pub options: ChatRequestOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideResponseResponse {}

// ----------------------------------------------------------------------------
// lm/responsePart (notification: backend -> vscode)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/responsePart")]
#[serde(rename_all = "camelCase")]
pub struct ResponsePartNotification {
    pub request_id: serde_json::Value,
    pub part: ContentPart,
}

// ----------------------------------------------------------------------------
// lm/responseComplete (notification: backend -> vscode)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/responseComplete")]
#[serde(rename_all = "camelCase")]
pub struct ResponseCompleteNotification {
    pub request_id: serde_json::Value,
}

// ----------------------------------------------------------------------------
// lm/cancel (notification: vscode -> backend)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/cancel")]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    pub request_id: serde_json::Value,
}

// ----------------------------------------------------------------------------
// lm/provideTokenCount
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideTokenCount", response = ProvideTokenCountResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProvideTokenCountRequest {
    pub model_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideTokenCountResponse {
    pub count: u32,
}

// ============================================================================
// Message Handler
// ============================================================================

/// Handler for LM backend messages.
/// Forwards requests to HistoryActor for actual processing.
pub struct LmBackendHandler {
    /// Handle to send messages to the HistoryActor.
    /// Created lazily on first request that needs it.
    history_handle: Option<HistoryActorHandle>,
}

impl LmBackendHandler {
    pub fn new() -> Self {
        Self {
            history_handle: None,
        }
    }

    /// Get or create the history actor handle.
    /// The actor is created lazily on first use, using the provided connection context.
    fn get_or_create_history_handle(
        &mut self,
        cx: &JrConnectionCx<LmBackendToVsCode>,
    ) -> Result<&HistoryActorHandle, sacp::Error> {
        if self.history_handle.is_none() {
            let handle = HistoryActor::new(&cx)?;
            self.history_handle = Some(handle);
        }
        Ok(self.history_handle.as_ref().unwrap())
    }
}

impl JrMessageHandler for LmBackendHandler {
    type Link = LmBackendToVsCode;

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "LmBackendHandler"
    }

    async fn handle_message(
        &mut self,
        message: MessageCx,
        cx: JrConnectionCx<Self::Link>,
    ) -> Result<Handled<MessageCx>, sacp::Error> {
        tracing::trace!(?message, "handle_message");

        // Get or create the history actor handle (lazy init on first call)
        let history_handle = self.get_or_create_history_handle(&cx)?.clone();

        MatchMessage::new(message)
            .if_request(async |_req: ProvideInfoRequest, request_cx| {
                let response = ProvideInfoResponse {
                    models: vec![ModelInfo {
                        id: "symposium-eliza".to_string(),
                        name: "Symposium (Eliza)".to_string(),
                        family: "symposium".to_string(),
                        version: "1.0.0".to_string(),
                        max_input_tokens: 100000,
                        max_output_tokens: 100000,
                        capabilities: ModelCapabilities { tool_calling: true },
                    }],
                };
                request_cx.respond(response)
            })
            .await
            .if_request(async |req: ProvideTokenCountRequest, request_cx| {
                // Simple heuristic: 1 token ≈ 4 characters
                let count = (req.text.len() / 4).max(1) as u32;
                request_cx.respond(ProvideTokenCountResponse { count })
            })
            .await
            .if_request(async |req: ProvideResponseRequest, request_cx| {
                tracing::debug!(?req, "ProvideResponseRequest");

                let request_id = request_cx.id().clone();

                // Forward to HistoryActor for processing
                history_handle.send_from_vscode(req, request_id, request_cx)?;

                Ok(())
            })
            .await
            .if_notification(async |notification: CancelNotification| {
                tracing::debug!(?notification, "CancelNotification");

                // Forward to HistoryActor
                history_handle.send_cancel_from_vscode(notification.request_id)?;

                Ok(())
            })
            .await
            .otherwise(async |message| match message {
                MessageCx::Request(request, request_cx) => {
                    tracing::warn!("unknown request method: {}", request.method());
                    request_cx.respond_with_error(sacp::Error::method_not_found())
                }
                MessageCx::Notification(notif) => {
                    tracing::warn!("unexpected notification: {}", notif.method());
                    Ok(())
                }
            })
            .await?;

        Ok(Handled::Yes)
    }
}

// ============================================================================
// Component Implementation
// ============================================================================

/// The LM backend component that can be used with sacp's Component infrastructure.
pub struct LmBackend {
    handler: LmBackendHandler,
}

impl LmBackend {
    pub fn new() -> Self {
        Self {
            handler: LmBackendHandler::new(),
        }
    }
}

impl Default for LmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl sacp::Component<LmBackendToVsCode> for LmBackend {
    async fn serve(
        self,
        client: impl sacp::Component<VsCodeToLmBackend>,
    ) -> Result<(), sacp::Error> {
        LmBackendToVsCode::builder()
            .with_handler(self.handler)
            .serve(client)
            .await
    }
}

// ============================================================================
// Server (for CLI usage)
// ============================================================================

/// Run the LM backend on stdio
pub async fn serve_stdio(trace_dir: Option<PathBuf>) -> Result<()> {
    let stdio = if let Some(dir) = trace_dir {
        std::fs::create_dir_all(&dir)?;
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let trace_path = dir.join(format!("vscodelm-{}.log", timestamp));
        let file = std::sync::Arc::new(std::sync::Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trace_path)?,
        ));
        tracing::info!(?trace_path, "Logging vscodelm messages");

        sacp_tokio::Stdio::new().with_debug(move |line, direction| {
            use std::io::Write;
            let dir_str = match direction {
                sacp_tokio::LineDirection::Stdin => "recv",
                sacp_tokio::LineDirection::Stdout => "send",
                sacp_tokio::LineDirection::Stderr => "stderr",
            };
            if let Ok(mut f) = file.lock() {
                let _ = writeln!(
                    f,
                    "[{}] {}: {}",
                    chrono::Utc::now().to_rfc3339(),
                    dir_str,
                    line
                );
                let _ = f.flush();
            }
        })
    } else {
        sacp_tokio::Stdio::new()
    };

    LmBackend::new().serve(stdio).await?;
    Ok(())
}
