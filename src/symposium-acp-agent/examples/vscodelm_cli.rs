//! Interactive CLI for debugging VS Code Language Model Provider integration.
//!
//! This example acts as a "fake VS Code" that connects to the vscodelm backend
//! with a real Claude Code agent. It allows interactive testing of the full
//! tool invocation flow.
//!
//! Usage:
//!   cargo run --example vscodelm_cli
//!   cargo run --example vscodelm_cli -- --log-file /tmp/vscodelm.log
//!
//! Then type prompts at the `> ` prompt. The example includes an "average" tool
//! that computes the average of a list of numbers - ask Claude to use it!
//!
//! Example prompts:
//!   > What is the average of 10, 20, 30, 40, 50?
//!   > Can you calculate the average of these test scores: 85, 92, 78, 95, 88?

use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use futures::channel::mpsc;
use sacp::{JrLink, on_receive_notification};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Parser, Debug)]
#[command(name = "vscodelm_cli")]
#[command(about = "Interactive CLI for debugging VS Code LM Provider integration")]
struct Args {
    /// Log file path. If provided, traces are written to this file.
    /// Use RUST_LOG to control log level (e.g., RUST_LOG=debug).
    #[arg(long)]
    log_file: Option<PathBuf>,
}

// Import vscodelm types - we need to make these pub or use a different approach
use symposium_acp_agent::vscodelm::session_actor::AgentDefinition;
use symposium_acp_agent::vscodelm::{
    ChatRequestOptions, ContentPart, LmBackend, Message, ProvideResponseRequest, ROLE_ASSISTANT,
    ROLE_USER, ResponseCompleteNotification, ResponsePartNotification, ToolDefinition, ToolMode,
    VsCodeToLmBackend,
};

/// The "average" tool that we provide to Claude Code.
fn average_tool() -> ToolDefinition {
    ToolDefinition {
        name: "average".to_string(),
        description: "Compute the arithmetic average of a list of numbers".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "numbers": {
                    "type": "array",
                    "items": { "type": "number" },
                    "description": "The list of numbers to average"
                }
            },
            "required": ["numbers"]
        }),
    }
}

/// Execute the average tool with the given parameters.
fn execute_average(params: &serde_json::Value) -> serde_json::Value {
    let numbers = params
        .get("numbers")
        .and_then(|n| n.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
        .unwrap_or_default();

    if numbers.is_empty() {
        return json!({"error": "No valid numbers provided"});
    }

    let sum: f64 = numbers.iter().sum();
    let avg = sum / numbers.len() as f64;

    json!({
        "average": avg,
        "count": numbers.len(),
        "sum": sum
    })
}

/// Collected response parts from the LM backend.
#[derive(Debug, Default)]
struct ResponseCollector {
    parts: Vec<ContentPart>,
}

impl ResponseCollector {
    fn push(&mut self, part: ContentPart) {
        self.parts.push(part);
    }

    fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { value } => Some(value.as_str()),
                _ => None,
            })
            .collect()
    }

    fn tool_calls(&self) -> Vec<(String, String, serde_json::Value)> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::ToolCall {
                    tool_call_id,
                    tool_name,
                    parameters,
                } => Some((tool_call_id.clone(), tool_name.clone(), parameters.clone())),
                _ => None,
            })
            .collect()
    }

    fn clear(&mut self) {
        self.parts.clear();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    if let Some(log_file) = args.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)?;
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::DEBUG.into()),
            )
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .init();
        eprintln!("Logging to: {}", log_file.display());
    } else if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    }

    println!("VS Code LM CLI - Interactive debugging tool");
    println!("============================================");
    println!("This connects to Claude Code via the vscodelm backend.");
    println!("An 'average' tool is available - try asking Claude to use it!");
    println!();
    println!("Example: What is the average of 10, 20, 30?");
    println!();
    println!("Type your prompts below. Ctrl+D to exit.");
    println!();

    let collector = Arc::new(Mutex::new(ResponseCollector::default()));
    let (complete_tx, mut complete_rx) = mpsc::unbounded::<()>();

    let collector_clone = collector.clone();
    let complete_tx_clone = complete_tx.clone();

    VsCodeToLmBackend::builder()
        .on_receive_notification(
            async move |n: ResponsePartNotification, _| {
                // Print text parts immediately for streaming effect
                if let ContentPart::Text { ref value } = n.part {
                    print!("{}", value);
                    io::stdout().flush().ok();
                }
                collector_clone.lock().unwrap().push(n.part);
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
            let stdin = io::stdin();
            let mut history: Vec<Message> = Vec::new();
            let tools = vec![average_tool()];

            loop {
                // Prompt for input
                print!("> ");
                io::stdout().flush().ok();

                let mut line = String::new();
                match stdin.lock().read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        // EOF or error
                        println!("\nGoodbye!");
                        break;
                    }
                    Ok(_) => {}
                }

                let prompt = line.trim();
                if prompt.is_empty() {
                    continue;
                }

                // Add user message to history
                history.push(Message {
                    role: ROLE_USER.to_string(),
                    content: vec![ContentPart::Text {
                        value: prompt.to_string(),
                    }],
                });

                // Clear collector for new response
                collector.lock().unwrap().clear();

                // Send request
                println!(); // Newline before response
                cx.send_request(ProvideResponseRequest {
                    model_id: "claude-code".to_string(),
                    messages: history.clone(),
                    agent: AgentDefinition::ClaudeCode,
                    options: ChatRequestOptions {
                        tools: tools.clone(),
                        tool_mode: Some(ToolMode::Auto),
                    },
                })
                .block_task()
                .await?;

                // Wait for response to complete
                complete_rx.next().await;
                println!(); // Newline after response

                // Check for tool calls
                let tool_calls = collector.lock().unwrap().tool_calls();
                let text = collector.lock().unwrap().text();

                // Add assistant response to history
                let mut assistant_content: Vec<ContentPart> = Vec::new();
                if !text.is_empty() {
                    assistant_content.push(ContentPart::Text { value: text });
                }
                for (id, name, params) in &tool_calls {
                    assistant_content.push(ContentPart::ToolCall {
                        tool_call_id: id.clone(),
                        tool_name: name.clone(),
                        parameters: params.clone(),
                    });
                }
                if !assistant_content.is_empty() {
                    history.push(Message {
                        role: ROLE_ASSISTANT.to_string(),
                        content: assistant_content,
                    });
                }

                // Process tool calls
                if !tool_calls.is_empty() {
                    for (tool_call_id, tool_name, params) in tool_calls {
                        println!("\n[Tool call: {} with {:?}]", tool_name, params);

                        let result = if tool_name == "average" {
                            execute_average(&params)
                        } else {
                            json!({"error": format!("Unknown tool: {}", tool_name)})
                        };

                        println!("[Tool result: {}]", result);

                        // Add tool result to history
                        history.push(Message {
                            role: ROLE_USER.to_string(),
                            content: vec![ContentPart::ToolResult {
                                tool_call_id: tool_call_id.clone(),
                                result,
                            }],
                        });
                    }

                    // Send follow-up request with tool results
                    collector.lock().unwrap().clear();
                    println!(); // Newline before continuation

                    cx.send_request(ProvideResponseRequest {
                        model_id: "claude-code".to_string(),
                        messages: history.clone(),
                        agent: AgentDefinition::ClaudeCode,
                        options: ChatRequestOptions {
                            tools: tools.clone(),
                            tool_mode: Some(ToolMode::Auto),
                        },
                    })
                    .block_task()
                    .await?;

                    // Wait for continuation response
                    complete_rx.next().await;
                    println!(); // Newline after response

                    // Add continuation to history
                    let continuation_text = collector.lock().unwrap().text();
                    if !continuation_text.is_empty() {
                        history.push(Message {
                            role: ROLE_ASSISTANT.to_string(),
                            content: vec![ContentPart::Text {
                                value: continuation_text,
                            }],
                        });
                    }
                }

                println!(); // Extra newline for readability
            }

            Ok(())
        })
        .await?;

    Ok(())
}
