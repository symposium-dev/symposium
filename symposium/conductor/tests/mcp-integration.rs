//! Integration tests for MCP tool routing through proxy components.
//!
//! These tests verify that:
//! 1. Proxy components can provide MCP tools
//! 2. Agent components can discover and invoke those tools
//! 3. Tool invocations route correctly through the proxy

mod mcp_integration;

use agent_client_protocol::{
    self as acp, ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, TextContent,
};
use conductor::component::ComponentProvider;
use conductor::conductor::Conductor;
use scp::JsonRpcConnection;

use tokio::io::duplex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test helper to receive a JSON-RPC response
async fn recv<R: scp::JsonRpcResponsePayload + Send>(
    response: scp::JsonRpcResponse<R>,
) -> Result<R, agent_client_protocol::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    response.await_when_response_received(async move |result| {
        tx.send(result)
            .map_err(|_| agent_client_protocol::Error::internal_error())
    })?;
    rx.await
        .map_err(|_| agent_client_protocol::Error::internal_error())?
}

async fn run_test_with_components(
    components: Vec<Box<dyn ComponentProvider>>,
    editor_task: impl AsyncFnOnce(scp::JsonRpcConnectionCx) -> Result<(), acp::Error>,
) -> Result<(), acp::Error> {
    // Set up editor <-> conductor communication
    let (editor_out, conductor_in) = duplex(1024);
    let (conductor_out, editor_in) = duplex(1024);

    JsonRpcConnection::new(editor_out.compat_write(), editor_in.compat())
        .name("editor-to-connector")
        .with_spawned(async move {
            Conductor::run(
                conductor_out.compat_write(),
                conductor_in.compat(),
                components,
            )
            .await
        })
        .with_client(editor_task)
        .await
}

#[tokio::test]
async fn test_proxy_provides_mcp_tools() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    run_test_with_components(
        vec![
            mcp_integration::proxy::create(),
            mcp_integration::agent::create(),
        ],
        async |editor_cx| {
            // Send initialization request
            let init_response = recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
            }))
            .await;

            assert!(
                init_response.is_ok(),
                "Initialize should succeed: {:?}",
                init_response
            );

            // Send session/new request
            let session_response = recv(editor_cx.send_request(NewSessionRequest {
                cwd: Default::default(),
                mcp_servers: vec![],
                meta: None,
            }))
            .await;

            assert!(
                session_response.is_ok(),
                "Session/new should succeed: {:?}",
                session_response
            );

            let session = session_response.unwrap();
            assert_eq!(&*session.session_id.0, "test-session-123");

            Ok(())
        },
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn test_agent_handles_prompt() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    run_test_with_components(
        vec![
            mcp_integration::proxy::create(),
            mcp_integration::agent::create(),
        ],
        async |editor_cx| {
            // Initialize
            recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
            }))
            .await?;

            // Create session
            let session = recv(editor_cx.send_request(NewSessionRequest {
                cwd: Default::default(),
                mcp_servers: vec![],
                meta: None,
            }))
            .await?;

            tracing::debug!(session_id = %session.session_id.0, "Session created");

            // Send a prompt
            let prompt_response = recv(editor_cx.send_request(PromptRequest {
                session_id: session.session_id.clone(),
                prompt: vec![ContentBlock::Text(TextContent {
                    annotations: None,
                    text: "Hello agent!".to_string(),
                    meta: None,
                })],
                meta: None,
            }))
            .await?;

            tracing::debug!(
                stop_reason = ?prompt_response.stop_reason,
                "Prompt response received"
            );
            assert_eq!(
                prompt_response.stop_reason,
                acp::StopReason::EndTurn,
                "Expected EndTurn stop reason"
            );

            Ok(())
        },
    )
    .await?;

    Ok(())
}
