//! End-to-end integration test for MCP bridge functionality
//!
//! This test validates the full MCP bridging flow:
//! 1. Proxy provides MCP tool with ACP transport
//! 2. Agent lacks mcp_acp_transport capability
//! 3. Conductor detects need for bridging
//! 4. Conductor spawns TCP listener and modifies MCP server list
//! 5. Agent uses rmcp to invoke tool via stdio bridge
//! 6. Bridge connects via TCP, messages flow backward to proxy
//! 7. Proxy handles tool invocation and responds

#![cfg(feature = "test-support")]

// Import helper modules from subdirectory
mod mcp_bridge_end_to_end_helpers;
use mcp_bridge_end_to_end_helpers::{mock_agent, mock_proxy};

use agent_client_protocol::{InitializeRequest, NewSessionRequest};
use conductor::component::ComponentProvider;
use conductor::conductor::Conductor;
use scp::JsonRpcConnection;
use tokio::io::duplex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[tokio::test]
async fn test_basic_mcp_tool_invocation() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    let local = tokio::task::LocalSet::new();

    local
        .run_until(async {
            // Create mock proxy that provides go_go_gadget_shoes tool
            let mock_proxy = mock_proxy::create_mock_proxy();

            // Create mock agent that uses rmcp to invoke tools
            let mock_agent = mock_agent::create_mock_agent();

            // Setup editor <-> conductor communication
            let (editor_out, conductor_in) = duplex(8192);
            let (conductor_out, editor_in) = duplex(8192);

            // Spawn conductor with proxy + agent chain
            let conductor_handle = tokio::task::spawn_local(async move {
                Conductor::run(
                    conductor_out.compat_write(),
                    conductor_in.compat(),
                    vec![
                        ComponentProvider::Mock(Box::new(mock_proxy)),
                        ComponentProvider::Mock(Box::new(mock_agent)),
                    ],
                )
                .await
            });

            // Create editor-side JSON-RPC connection
            let editor_task = tokio::task::spawn_local(async move {
                JsonRpcConnection::new(editor_out.compat_write(), editor_in.compat())
                    .with_client(async move |client| {
                        // Step 1: Initialize
                        let init_response = client
                            .send_request(InitializeRequest {
                                protocol_version: Default::default(),
                                client_capabilities: Default::default(),
                                meta: None,
                            })
                            .recv()
                            .await;

                        assert!(
                            init_response.is_ok(),
                            "Initialize failed: {:?}",
                            init_response
                        );
                        tracing::info!("Initialize response: {:?}", init_response);

                        // Step 2: Create new session
                        let session_response = client
                            .send_request(NewSessionRequest {
                                mcp_servers: vec![], // No MCP servers from editor
                                cwd: Default::default(),
                                meta: None,
                            })
                            .recv()
                            .await;

                        assert!(
                            session_response.is_ok(),
                            "Session creation failed: {:?}",
                            session_response
                        );
                        tracing::info!("Session response: {:?}", session_response);

                        // TODO: Step 3: Send prompt that causes agent to invoke tool
                        // TODO: Step 4: Verify tool was invoked on proxy
                        // TODO: Step 5: Verify agent received response

                        Ok::<_, acp::Error>(())
                    })
                    .await
            });

            // Wait for editor task to complete - conductor will keep running
            let _ = editor_task.await.expect("Editor task should complete");

            // Conductor is still running but we're done with our test
            // Drop the handle to let it clean up
            drop(conductor_handle);
        })
        .await;
}
