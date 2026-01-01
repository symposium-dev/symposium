//! VS Code Language Model Provider backend
//!
//! This module implements the Rust backend for the VS Code `LanguageModelChatProvider` API.
//! It uses sacp's JSON-RPC infrastructure for communication with the TypeScript extension.

use anyhow::Result;
use elizacp::eliza::Eliza;
use sacp::{
    link::RemoteStyle, util::MatchMessage, Component, Handled, JrConnectionCx, JrLink,
    JrMessageHandler, JrNotification, JrPeer, JrRequest, JrResponsePayload, MessageCx,
};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentPart {
    Text { value: String },
}

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            })
            .collect::<Vec<_>>()
            .join("")
    }
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
    pub part: ResponsePart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponsePart {
    Text { value: String },
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

/// Handler for LM backend messages
pub struct LmBackendHandler {
    eliza: Eliza,
}

impl LmBackendHandler {
    pub fn new() -> Self {
        Self {
            eliza: Eliza::new(),
        }
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

                // Get the request ID from the request context for notifications
                let request_id = request_cx.id().clone();

                // Extract the last user message
                let user_message = req
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user")
                    .map(|m| m.text())
                    .unwrap_or_default();

                // Generate response from Eliza
                let response_text = if user_message.is_empty() {
                    self.eliza.hello().to_string()
                } else {
                    self.eliza.respond(&user_message)
                };

                tracing::debug!(?response_text, "eliza response");

                // Stream the response in chunks
                for chunk in response_text.chars().collect::<Vec<_>>().chunks(5) {
                    let chunk_str: String = chunk.iter().collect();
                    cx.send_notification(ResponsePartNotification {
                        request_id: request_id.clone(),
                        part: ResponsePart::Text { value: chunk_str },
                    })?;
                }

                // Send completion notification
                cx.send_notification(ResponseCompleteNotification { request_id })?;

                // Send the response
                request_cx.respond(ProvideResponseResponse {})
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

    /// Create a new LmBackend with deterministic Eliza responses (for testing).
    #[cfg(test)]
    pub fn new_deterministic() -> Self {
        Self {
            handler: LmBackendHandler {
                eliza: Eliza::new_deterministic(),
            },
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
pub async fn serve_stdio() -> Result<()> {
    LmBackend::new().serve(sacp_tokio::Stdio::new()).await?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    #[tokio::test]
    async fn test_provide_info() -> Result<(), sacp::Error> {
        VsCodeToLmBackend::builder()
            .connect_to(LmBackend::new_deterministic())?
            .run_until(async |cx| {
                let response = cx
                    .send_request(ProvideInfoRequest { silent: false })
                    .block_task()
                    .await?;

                expect![[r#"
                    ProvideInfoResponse {
                        models: [
                            ModelInfo {
                                id: "symposium-eliza",
                                name: "Symposium (Eliza)",
                                family: "symposium",
                                version: "1.0.0",
                                max_input_tokens: 100000,
                                max_output_tokens: 100000,
                                capabilities: ModelCapabilities {
                                    tool_calling: true,
                                },
                            },
                        ],
                    }
                "#]]
                .assert_debug_eq(&response);

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_provide_token_count() -> Result<(), sacp::Error> {
        VsCodeToLmBackend::builder()
            .connect_to(LmBackend::new_deterministic())?
            .run_until(async |cx| {
                let response = cx
                    .send_request(ProvideTokenCountRequest {
                        model_id: "symposium-eliza".to_string(),
                        text: "Hello, world!".to_string(),
                    })
                    .block_task()
                    .await?;

                expect![[r#"
                    ProvideTokenCountResponse {
                        count: 3,
                    }
                "#]]
                .assert_debug_eq(&response);

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_chat_response() -> Result<(), sacp::Error> {
        let mut full_response = String::new();

        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async |notif: ResponsePartNotification, _cx| {
                    let ResponsePart::Text { value } = notif.part;
                    full_response.push_str(&value);
                    Ok(())
                },
                sacp::on_receive_notification!(),
            )
            .connect_to(LmBackend::new_deterministic())?
            .run_until(async |cx| {
                cx.send_request(ProvideResponseRequest {
                    model_id: "symposium-eliza".to_string(),
                    messages: vec![Message {
                        role: "user".to_string(),
                        content: vec![ContentPart::Text {
                            value: "I feel happy".to_string(),
                        }],
                    }],
                })
                .block_task()
                .await?;

                Ok(())
            })
            .await?;

        expect![[r#"Do you often feel happy?"#]].assert_eq(&full_response);

        Ok(())
    }
}
