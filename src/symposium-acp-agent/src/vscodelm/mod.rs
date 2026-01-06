//! VS Code Language Model Provider backend
//!
//! This module implements the Rust backend for the VS Code `LanguageModelChatProvider` API.
//! It uses sacp's JSON-RPC infrastructure for communication with the TypeScript extension.

mod history_actor;
mod session_actor;
mod vscode_tools_mcp;

use anyhow::Result;
use history_actor::{HistoryActor, HistoryActorHandle};
use sacp::{
    link::RemoteStyle, util::MatchMessage, Component, Handled, JrConnectionCx, JrLink,
    JrMessageHandler, JrNotification, JrPeer, JrRequest, JrResponsePayload, MessageCx,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Name of the special tool we inject into vscode for requesting permission
const SYMPOSIUM_AGENT_ACTION: &str = "symposium-agent-action";

/// Role constants for message matching
pub(crate) const ROLE_USER: &str = "user";
pub(crate) const ROLE_ASSISTANT: &str = "assistant";

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
            .connect_to(LmBackend::new())?
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
            .connect_to(LmBackend::new())?
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

    // TODO: Add integration tests that spawn a real agent process
    // The chat_response and session_continuation tests have been removed
    // because they relied on the old in-process Eliza implementation.
    // With the new architecture, the session actor spawns an external
    // ACP agent process, which requires different test infrastructure.

    #[test]
    fn test_chat_request_options_deserialization() {
        // Test deserializing options from TypeScript format
        let json = r#"{
            "tools": [
                {
                    "name": "symposium-agent-action",
                    "description": "Request permission for agent actions",
                    "inputSchema": {"type": "object", "properties": {"action": {"type": "string"}}}
                }
            ],
            "toolMode": "auto"
        }"#;

        let options: ChatRequestOptions = serde_json::from_str(json).unwrap();
        assert_eq!(options.tools.len(), 1);
        assert_eq!(options.tools[0].name, "symposium-agent-action");
        assert_eq!(options.tool_mode, Some(ToolMode::Auto));
    }

    #[test]
    fn test_chat_request_options_default() {
        // Test that missing options deserialize to defaults
        let json = r#"{}"#;
        let options: ChatRequestOptions = serde_json::from_str(json).unwrap();
        assert!(options.tools.is_empty());
        assert_eq!(options.tool_mode, None);
    }

    #[test]
    fn test_agent_definition_eliza_serialization() {
        use super::session_actor::AgentDefinition;

        let agent = AgentDefinition::Eliza {
            deterministic: true,
        };
        let json = serde_json::to_string_pretty(&agent).unwrap();
        println!("Eliza:\n{}", json);

        // Should serialize as {"eliza": {"deterministic": true}}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("eliza").is_some());
        assert_eq!(parsed["eliza"]["deterministic"], true);
    }

    #[test]
    fn test_agent_definition_mcp_server_serialization() {
        use super::session_actor::AgentDefinition;
        use sacp::schema::{McpServer, McpServerStdio};

        let server = McpServer::Stdio(McpServerStdio::new("test", "echo"));
        let agent = AgentDefinition::McpServer(server);
        let json = serde_json::to_string_pretty(&agent).unwrap();
        println!("McpServer:\n{}", json);

        // Should serialize as {"mcp_server": {name, command, args, env}}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("mcp_server").is_some());
        assert_eq!(parsed["mcp_server"]["name"], "test");
        assert_eq!(parsed["mcp_server"]["command"], "echo");
    }

    // ========================================================================
    // Integration tests with elizacp
    // ========================================================================

    use super::session_actor::AgentDefinition;
    use futures::channel::mpsc;
    use futures::StreamExt;
    use sacp::on_receive_notification;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Collected notifications from the LM backend.
    #[derive(Debug, Default, Clone)]
    struct CollectedParts(Vec<ContentPart>);

    impl CollectedParts {
        /// Extract just the text content, concatenated.
        fn text(&self) -> String {
            self.0
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { value } => Some(value.as_str()),
                    _ => None,
                })
                .collect()
        }

        /// Extract tool calls as (tool_name, parameters_json) pairs.
        fn tool_calls(&self) -> Vec<(String, String)> {
            self.0
                .iter()
                .filter_map(|p| match p {
                    ContentPart::ToolCall {
                        tool_name,
                        parameters,
                        ..
                    } => Some((tool_name.clone(), parameters.to_string())),
                    _ => None,
                })
                .collect()
        }

        /// Extract tool calls as (tool_call_id, tool_name, parameters_json) tuples.
        fn tool_calls_with_ids(&self) -> Vec<(String, String, String)> {
            self.0
                .iter()
                .filter_map(|p| match p {
                    ContentPart::ToolCall {
                        tool_call_id,
                        tool_name,
                        parameters,
                    } => Some((
                        tool_call_id.clone(),
                        tool_name.clone(),
                        parameters.to_string(),
                    )),
                    _ => None,
                })
                .collect()
        }

        fn clear(&mut self) {
            self.0.clear();
        }
    }

    /// Helper to send a chat request with elizacp.
    async fn send_chat(
        cx: &sacp::JrConnectionCx<VsCodeToLmBackend>,
        prompt: &str,
        tools: Vec<ToolDefinition>,
    ) -> Result<(), sacp::Error> {
        let messages = vec![Message {
            role: ROLE_USER.to_string(),
            content: vec![ContentPart::Text {
                value: prompt.to_string(),
            }],
        }];

        cx.send_request(ProvideResponseRequest {
            model_id: "symposium-eliza".to_string(),
            messages,
            agent: AgentDefinition::Eliza {
                deterministic: true,
            },
            options: ChatRequestOptions {
                tools,
                tool_mode: Some(ToolMode::Auto),
            },
        })
        .block_task()
        .await?;

        Ok(())
    }

    /// Test that a simple chat request with elizacp works end-to-end.
    #[tokio::test]
    async fn test_simple_chat_request() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                send_chat(&cx, "Hello, how are you?", vec![]).await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                expect!["I don't have feelings, but I'm functioning well. What about you?"]
                    .assert_eq(&parts.lock().unwrap().text());
                Ok(())
            })
            .await
    }

    /// Test that tools provided in the request are passed through correctly.
    #[tokio::test]
    async fn test_chat_request_with_tools() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let tools = vec![
                    ToolDefinition {
                        name: "test_read_file".to_string(),
                        description: "Read a file".to_string(),
                        input_schema: serde_json::json!({"type": "object"}),
                    },
                    ToolDefinition {
                        name: "test_write_file".to_string(),
                        description: "Write a file".to_string(),
                        input_schema: serde_json::json!({"type": "object"}),
                    },
                ];

                send_chat(&cx, "Hello", tools).await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                // Eliza responds regardless of tools
                expect!["How do you do. Please state your problem."]
                    .assert_eq(&parts.lock().unwrap().text());
                Ok(())
            })
            .await
    }

    /// Test multi-turn conversation maintains session state.
    #[tokio::test]
    async fn test_multi_turn_conversation() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                // First turn
                send_chat(&cx, "Hello", vec![]).await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                expect!["How do you do. Please state your problem."]
                    .assert_eq(&parts.lock().unwrap().text());
                parts.lock().unwrap().clear();

                // Second turn - send full history
                let messages = vec![
                    Message {
                        role: ROLE_USER.to_string(),
                        content: vec![ContentPart::Text {
                            value: "Hello".to_string(),
                        }],
                    },
                    Message {
                        role: ROLE_ASSISTANT.to_string(),
                        content: vec![ContentPart::Text {
                            value: "How do you do. Please state your problem.".to_string(),
                        }],
                    },
                    Message {
                        role: ROLE_USER.to_string(),
                        content: vec![ContentPart::Text {
                            value: "I am doing well, thanks!".to_string(),
                        }],
                    },
                ];

                cx.send_request(ProvideResponseRequest {
                    model_id: "symposium-eliza".to_string(),
                    messages,
                    agent: AgentDefinition::Eliza {
                        deterministic: true,
                    },
                    options: ChatRequestOptions::default(),
                })
                .block_task()
                .await?;

                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                // Eliza responds to the second turn
                expect!["Do you believe it is normal to be doing well thanks?"]
                    .assert_eq(&parts.lock().unwrap().text());
                Ok(())
            })
            .await
    }

    /// Test that elizacp can list VS Code tools via the MCP bridge.
    ///
    /// Uses elizacp's "list tools from <server>" command to verify
    /// that tools provided in the request are visible via MCP.
    #[tokio::test]
    async fn test_mcp_list_tools() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let tools = vec![
                    ToolDefinition {
                        name: "read_file".to_string(),
                        description: "Read contents of a file".to_string(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {"path": {"type": "string"}},
                            "required": ["path"]
                        }),
                    },
                    ToolDefinition {
                        name: "write_file".to_string(),
                        description: "Write contents to a file".to_string(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
                            "required": ["path", "content"]
                        }),
                    },
                ];

                send_chat(&cx, "list tools from vscode-tools", tools).await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                expect![[r#"
                    Available tools:
                      - read_file: Read contents of a file
                      - write_file: Write contents to a file"#]]
                .assert_eq(&parts.lock().unwrap().text());

                Ok(())
            })
            .await
    }

    /// Test that elizacp can invoke VS Code tools via the MCP bridge.
    ///
    /// Uses elizacp's "use tool <server>::<tool> with <json>" command.
    /// Verifies that the tool invocation triggers a ToolCall back to VS Code.
    #[tokio::test]
    async fn test_mcp_invoke_tool() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let tools = vec![ToolDefinition {
                    name: "read_file".to_string(),
                    description: "Read contents of a file".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }),
                }];

                send_chat(
                    &cx,
                    r#"use tool vscode-tools::read_file with {"path": "/tmp/test.txt"}"#,
                    tools,
                )
                .await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout");

                // Should have received a ToolCall part
                let tool_calls = parts.lock().unwrap().tool_calls();
                expect![[r#"
                    [
                        (
                            "read_file",
                            "{\"path\":\"/tmp/test.txt\"}",
                        ),
                    ]
                "#]]
                .assert_debug_eq(&tool_calls);

                Ok(())
            })
            .await
    }

    /// Test that providing a tool result causes elizacp to echo it back.
    #[tokio::test]
    async fn test_mcp_tool_result_flow() -> Result<(), sacp::Error> {
        let parts = Arc::new(Mutex::new(CollectedParts::default()));
        let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

        let parts_clone = parts.clone();
        let complete_tx_clone = complete_tx.clone();
        VsCodeToLmBackend::builder()
            .on_receive_notification(
                async move |n: ResponsePartNotification, _| {
                    parts_clone.lock().unwrap().0.push(n.part);
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_notification(
                async move |_: ResponseCompleteNotification, _| {
                    let _ = complete_tx_clone.unbounded_send(());
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let tools = vec![ToolDefinition {
                    name: "read_file".to_string(),
                    description: "Read contents of a file".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }),
                }];

                // First request: trigger a tool call
                send_chat(
                    &cx,
                    r#"use tool vscode-tools::read_file with {"path": "/tmp/test.txt"}"#,
                    tools.clone(),
                )
                .await?;
                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout waiting for tool call");

                // Extract the tool call ID
                let tool_calls = parts.lock().unwrap().tool_calls_with_ids();
                assert_eq!(tool_calls.len(), 1, "expected exactly one tool call");
                let (tool_call_id, tool_name, _params) = &tool_calls[0];
                assert_eq!(tool_name, "read_file");
                let tool_call_id = tool_call_id.clone();
                parts.lock().unwrap().clear();

                // Second request: provide the tool result
                let messages = vec![
                    Message {
                        role: ROLE_USER.to_string(),
                        content: vec![ContentPart::Text {
                            value:
                                r#"use tool vscode-tools::read_file with {"path": "/tmp/test.txt"}"#
                                    .to_string(),
                        }],
                    },
                    Message {
                        role: ROLE_ASSISTANT.to_string(),
                        content: vec![ContentPart::ToolCall {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: "read_file".to_string(),
                            parameters: serde_json::json!({"path": "/tmp/test.txt"}),
                        }],
                    },
                    Message {
                        role: ROLE_USER.to_string(),
                        content: vec![ContentPart::ToolResult {
                            tool_call_id: tool_call_id.clone(),
                            result: serde_json::json!("Hello from the file!"),
                        }],
                    },
                ];

                cx.send_request(ProvideResponseRequest {
                    model_id: "symposium-eliza".to_string(),
                    messages,
                    agent: AgentDefinition::Eliza {
                        deterministic: true,
                    },
                    options: ChatRequestOptions {
                        tools,
                        tool_mode: Some(ToolMode::Auto),
                    },
                })
                .block_task()
                .await?;

                tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                    .await
                    .expect("timeout waiting for response after tool result");

                // Eliza should echo back the tool result
                let response_text = parts.lock().unwrap().text();
                expect![[r#"OK: CallToolResult { content: [Annotated { raw: Text(RawTextContent { text: "Hello from the file!", meta: None }), annotations: None }], structured_content: None, is_error: Some(false), meta: None }"#]].assert_eq(&response_text);

                Ok(())
            })
            .await
    }
}
