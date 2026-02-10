//! Tests for the VS Code Language Model Provider.

use super::*;
use expect_test::expect;

/// Initialize tracing for tests. Call at the start of tests that need logging.
/// Set RUST_LOG=trace (or debug, info, etc.) to see output.
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .with_test_writer()
        .try_init();
}

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

// ============================================================================
// Integration tests with elizacp
// ============================================================================

use super::session_actor::AgentDefinition;
use futures::StreamExt;
use futures::channel::mpsc;
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

            let text = parts.lock().unwrap().text();
            expect!["How do you do. Please state your problem."].assert_eq(&text);
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
                        value: text.to_string(),
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

            send_chat(&cx, "list tools from vscode_tools", tools).await?;
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
/// Verifies that the tool invocation triggers a ToolCall back to VS Code,
/// and that sending a ToolResult allows Eliza to continue with the response.
#[tokio::test]
async fn test_mcp_invoke_tool() -> Result<(), sacp::Error> {
    init_tracing();

    let parts = Arc::new(Mutex::new(CollectedParts::default()));
    let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

    let parts_clone = parts.clone();
    let complete_tx_clone = complete_tx.clone();
    VsCodeToLmBackend::builder()
        .on_receive_notification(
            async move |n: ResponsePartNotification, _| {
                tracing::debug!(?n.part, "received response part");
                parts_clone.lock().unwrap().0.push(n.part);
                Ok(())
            },
            on_receive_notification!(),
        )
        .on_receive_notification(
            async move |_: ResponseCompleteNotification, _| {
                tracing::debug!("received response complete");
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

            // Step 1: Send the initial chat that triggers a tool use
            tracing::info!("Step 1: sending tool use command");
            send_chat(
                &cx,
                r#"use tool vscode_tools::read_file with {"path": "/tmp/test.txt"}"#,
                tools.clone(),
            )
            .await?;
            tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                .await
                .expect("timeout waiting for tool call");

            // Should have received a ToolCall part - extract the tool_call_id
            let tool_calls = parts.lock().unwrap().tool_calls_with_ids();
            assert_eq!(tool_calls.len(), 1, "expected exactly one tool call");
            let (tool_call_id, tool_name, params) = &tool_calls[0];
            assert_eq!(tool_name, "read_file");
            tracing::info!(%tool_call_id, "received tool call, sending result");

            // Step 2: Send the tool result back with full history
            // This simulates VS Code executing the tool and returning the result.
            // Like multi-turn conversation, we need to send the full history:
            // 1. Original user message
            // 2. Assistant's tool call
            // 3. User's tool result
            let messages = vec![
                // Original user message
                Message {
                    role: ROLE_USER.to_string(),
                    content: vec![ContentPart::Text {
                        value: r#"use tool vscode_tools::read_file with {"path": "/tmp/test.txt"}"#
                            .to_string(),
                    }],
                },
                // Assistant's tool call response
                Message {
                    role: ROLE_ASSISTANT.to_string(),
                    content: vec![ContentPart::ToolCall {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        parameters: serde_json::from_str(params).unwrap(),
                    }],
                },
                // User's tool result
                Message {
                    role: ROLE_USER.to_string(),
                    content: vec![ContentPart::ToolResult {
                        tool_call_id: tool_call_id.clone(),
                        result: serde_json::json!("Hello from the file!"),
                    }],
                },
            ];

            parts.lock().unwrap().clear();

            tracing::info!("Step 2: sending tool result");
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

            // Wait for Eliza to complete its response after receiving the tool result
            tracing::info!("waiting for final response");
            tokio::time::timeout(Duration::from_secs(10), complete_rx.next())
                .await
                .expect("timeout waiting for final response");

            // Eliza should have responded with the tool result.
            // elizacp formats tool results as "OK: CallToolResult { ... }"
            let final_text = parts.lock().unwrap().text();
            tracing::info!(%final_text, "got final response");
            expect![[r#"OK: CallToolResult { content: [Annotated { raw: Text(RawTextContent { text: "Hello from the file!", meta: None }), annotations: None }], structured_content: None, is_error: Some(false), meta: None }"#]]
                .assert_eq(&final_text);

            Ok(())
        })
        .await
}
