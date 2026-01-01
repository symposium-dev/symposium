//! VS Code Language Model Provider backend
//!
//! This module implements a JSON-RPC server that backs the VS Code
//! `LanguageModelChatProvider` API. It receives requests from the TypeScript
//! extension and uses Eliza to generate responses (for the prototype).

use anyhow::Result;
use elizacp::eliza::Eliza;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

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

/// Parameters for lm/provideLanguageModelChatResponse
#[derive(Debug, Deserialize)]
pub struct ProvideResponseParams {
    pub messages: Vec<Message>,
}

/// The vscodelm server state
pub struct VsCodeLmServer {
    eliza: Eliza,
}

impl VsCodeLmServer {
    pub fn new() -> Self {
        Self {
            eliza: Eliza::new(),
        }
    }

    /// Run the JSON-RPC server on stdio
    pub async fn serve(self) -> Result<()> {
        let server = Arc::new(Mutex::new(self));
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let mut reader = BufReader::new(stdin);
        let mut stdout = stdout;
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                // EOF
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            tracing::debug!("received: {}", line);

            // Parse as JSON-RPC
            let msg: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("failed to parse JSON: {}", e);
                    continue;
                }
            };

            // Handle the message
            let responses = server.lock().await.handle_message(msg).await;

            // Send responses
            for response in responses {
                let response_str = serde_json::to_string(&response)?;
                tracing::debug!("sending: {}", response_str);
                stdout.write_all(response_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        Ok(())
    }

    /// Handle a JSON-RPC message, returning responses to send
    async fn handle_message(&mut self, msg: serde_json::Value) -> Vec<serde_json::Value> {
        let mut responses = Vec::new();

        let method = msg.get("method").and_then(|v| v.as_str());
        let id = msg.get("id").cloned();
        let params = msg
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        match method {
            Some("lm/provideLanguageModelChatResponse") => {
                let request_id = match &id {
                    Some(id) => id.clone(),
                    None => {
                        tracing::error!("missing id for request");
                        return responses;
                    }
                };

                // Parse params
                let params: ProvideResponseParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("failed to parse params: {}", e);
                        responses.push(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "error": {
                                "code": -32602,
                                "message": format!("Invalid params: {}", e)
                            }
                        }));
                        return responses;
                    }
                };

                // Extract the last user message
                let user_message = params
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user")
                    .map(|m| m.text())
                    .unwrap_or_default();

                tracing::info!("user message: {}", user_message);

                // Generate response from Eliza
                let response_text = if user_message.is_empty() {
                    self.eliza.hello().to_string()
                } else {
                    self.eliza.respond(&user_message)
                };

                tracing::info!("eliza response: {}", response_text);

                // Stream the response character by character (to exercise streaming)
                // In a real implementation, we'd stream larger chunks
                for chunk in response_text.chars().collect::<Vec<_>>().chunks(5) {
                    let chunk_str: String = chunk.iter().collect();
                    responses.push(serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "lm/responsePart",
                        "params": {
                            "requestId": request_id,
                            "part": {
                                "type": "text",
                                "value": chunk_str
                            }
                        }
                    }));
                }

                // Send completion notification
                responses.push(serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "lm/responseComplete",
                    "params": {
                        "requestId": request_id
                    }
                }));

                // Send the response
                responses.push(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {}
                }));
            }
            Some("lm/provideLanguageModelChatInformation") => {
                if let Some(request_id) = id {
                    responses.push(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": [{
                            "id": "symposium-eliza",
                            "name": "Symposium (Eliza)",
                            "family": "symposium",
                            "version": "1.0.0",
                            "maxInputTokens": 100000,
                            "maxOutputTokens": 100000,
                            "capabilities": {
                                "toolCalling": false
                            }
                        }]
                    }));
                }
            }
            Some("lm/provideTokenCount") => {
                if let Some(request_id) = id {
                    // Simple heuristic: 1 token ≈ 4 characters
                    let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    let token_count = (text.len() / 4).max(1);
                    responses.push(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": token_count
                    }));
                }
            }
            Some(method) => {
                tracing::warn!("unknown method: {}", method);
                if let Some(request_id) = id {
                    responses.push(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "error": {
                            "code": -32601,
                            "message": format!("Method not found: {}", method)
                        }
                    }));
                }
            }
            None => {
                tracing::warn!("message without method");
            }
        }

        responses
    }
}
