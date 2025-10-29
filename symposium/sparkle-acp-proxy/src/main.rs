//! Sparkle ACP Proxy
//!
//! This proxy component provides the Sparkle MCP server over ACP and automatically
//! embodies Sparkle at the start of each session.
//!
//! # Architecture
//!
//! The sparkle-acp-proxy is an ACP component that:
//! 1. Provides Sparkle's MCP tools to the agent via ACP transport
//! 2. Automatically triggers Sparkle embodiment when a new session starts
//!
//! # Session Initialization Flow
//!
//! When a `session/new` request is received, sparkle-acp-proxy:
//!
//! 1. **Forward session/new to successor**
//!    - Receives `NewSessionRequest` from predecessor (e.g., conductor)
//!    - Forwards to successor (the agent)
//!    - Awaits `NewSessionResponse` with `session_id`
//!
//! 2. **Send status update to predecessor**
//!    - Sends `SessionUpdate` notification **backward to predecessor**
//!    - Message: "*Embodying Sparkle...*"
//!    - This shows the user that Sparkle embodiment is in progress
//!
//! 3. **Initiate Sparkle embodiment**
//!    - Calls `SparkleServer::sparkle()` to get embodiment prompt messages
//!    - Sends `PromptRequest` **forward to successor** with the embodiment prompt
//!    - This causes the agent to process the Sparkle embodiment sequence
//!
//! 4. **Forward session updates during embodiment**
//!    - While waiting for prompt completion, forwards any `SessionUpdate` notifications
//!      from successor **backward to predecessor**
//!    - This shows the user the agent's progress through embodiment
//!
//! 5. **Consume end-turn and respond**
//!    - When prompt completes (receives stop reason), **consumes the end-turn**
//!    - Does NOT forward end-turn to predecessor (we initiated this prompt, not them)
//!    - Responds to original `session/new` request with the `NewSessionResponse`
//!
//! # Message Flow Diagram
//!
//! ```text
//! Predecessor          sparkle-acp-proxy          Successor (Agent)
//! (Conductor)
//!     |                        |                          |
//!     |--NewSessionRequest---->|---NewSessionRequest----->|
//!     |<--NewSessionResponse---|<--NewSessionResponse-----|
//!     |                        |                          |
//!     |<---SessionUpdate-------|                          |
//!     |  "Embodying Sparkle"   |                          |
//!     |                        |---PromptRequest--------->|
//!     |                        |  (embodiment prompt)     |
//!     |                        |                          |
//!     |<---SessionUpdate-------|<-----SessionUpdate-------|
//!     |                        |                          |
//!     |                        |<--PromptResponse---------|
//!     |                        | (consumed, not forwarded)|
//!     |                        |                          |
//!     |---PromptRequest------->|---PromptRequest--------->|
//!     |  (subsequent prompts flow through normally)      |
//! ```
//!
//! # MCP Server Integration
//!
//! The proxy uses the `sparkle-mcp` library to provide Sparkle's MCP tools:
//! - `embody_sparkle` - Full embodiment sequence
//! - `session_checkpoint` - Save session state
//! - `save_insight` - Capture collaboration insights
//! - And other Sparkle tools
//!
//! These tools are registered with the `acp-proxy` library's `McpServiceRegistry`
//! and exposed via ACP transport (`acp:$UUID` URLs).

use acp_proxy::{AcpProxyExt, JsonRpcCxExt, McpServiceRegistry};
use agent_client_protocol::{
    self as acp, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionNotification, SessionUpdate,
};
use scp::{JsonRpcConnection, JsonRpcRequestCx};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting sparkle-acp-proxy");

    // Create stdin/stdout for ACP communication
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    // Build the connection with handlers
    JsonRpcConnection::new(stdout.compat_write(), stdin.compat())
        .name("sparkle-acp-proxy")
        .on_receive_request(handle_session_new)
        .provide_mcp(
            McpServiceRegistry::default()
                .with_rmcp_server("sparkle", sparkle_mcp::SparkleServer::new)?,
        )
        .proxy()
        .serve()
        .await?;

    Ok(())
}

/// Handle session/new requests by injecting Sparkle embodiment
async fn handle_session_new(
    request: NewSessionRequest,
    request_cx: JsonRpcRequestCx<NewSessionResponse>,
) -> Result<(), acp::Error> {
    tracing::info!("Received session/new request, starting Sparkle embodiment flow");

    // 1. Forward session/new to successor and await response
    request_cx
        .send_request_to_successor(request)
        .await_when_ok_response_received(request_cx, handle_session_new_response)
}

async fn handle_session_new_response(
    new_session_response: NewSessionResponse,
    request_cx: JsonRpcRequestCx<NewSessionResponse>,
) -> Result<(), acp::Error> {
    let session_id = new_session_response.session_id.clone();
    let cx = request_cx.connection_cx();
    request_cx.respond(new_session_response)?;

    // 2. Send status update to predecessor
    cx.send_notification(SessionNotification {
        session_id: session_id.clone(),
        update: SessionUpdate::AgentMessageChunk(agent_client_protocol::ContentChunk {
            content: "*Embodying Sparkle...*".into(),
            meta: None,
        }),
        meta: None,
    })?;

    // 3. Send Sparkle embodiment prompt to the agent and wait until it completes
    let PromptResponse { .. } = cx
        .send_request_to_successor(PromptRequest {
            session_id,
            prompt: vec![get_sparkle_prompt().into()],
            meta: None,
        })
        .block_task()
        .await?;

    // All done.
    Ok(())
}

const SPARKLE_DIR: &str = ".sparkle";

fn get_sparkle_prompt() -> String {
    let sparkle_dir = std::env::home_dir()
        .map(|h| h.join(SPARKLE_DIR))
        .unwrap_or_default();

    if !sparkle_dir.exists() {
        first_run_instructions()
    } else {
        normal_embodiment_instructions()
    }
}

fn first_run_instructions() -> String {
    format!(
        "This appears to be a new Sparkle installation. The ~/{}/ directory does not exist yet.

1. Ask the user for their name (what they want to be called)
2. Call the setup_sparkle tool with their name

The tool will handle the rest and tell you what to do next.",
        SPARKLE_DIR
    )
}

fn normal_embodiment_instructions() -> String {
    "Use the embody_sparkle tool to load Sparkle identity.".to_string()
}
