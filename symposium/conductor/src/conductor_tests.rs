use std::sync::Arc;

use agent_client_protocol::ContentBlock;
use agent_client_protocol::{InitializeRequest, InitializeResponse};
use scp::{AcpAgentToClientCallbacks, JsonRpcCxExt};
use scp::{AcpClientToAgentCallbacks, AcpClientToAgentMessages, JsonRpcConnection, JsonRpcCx};
use tokio::{io::duplex, sync::Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::Instrument;

use crate::{
    component::{ComponentProvider, MockComponentImpl},
    conductor::Conductor,
};

/// Helper to create a mock component that captures initialize requests.
fn capturing_mock_component() -> (MockComponentImpl, Arc<Mutex<Option<InitializeRequest>>>) {
    let captured_init = Arc::new(Mutex::new(None));
    let captured_init_clone = captured_init.clone();

    let mock = MockComponentImpl::new(move |connection| async move {
        let _ = connection
            .on_receive(AcpClientToAgentMessages::callback(CapturingCallbacks {
                captured_init: captured_init_clone,
            }))
            .serve()
            .instrument(tracing::info_span!("actor", id = "mock_component"))
            .await;
    });

    (mock, captured_init)
}

/// Callbacks that capture initialize requests and respond
struct CapturingCallbacks {
    captured_init: Arc<Mutex<Option<InitializeRequest>>>,
}

impl AcpClientToAgentCallbacks for CapturingCallbacks {
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: scp::JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        // Capture the request for test verification
        *self.captured_init.lock().await = Some(args);

        let _ = response.respond(InitializeResponse {
            protocol_version: Default::default(),
            agent_capabilities: Default::default(),
            auth_methods: vec![],
            meta: None,
        });
        Ok(())
    }

    async fn authenticate(
        &mut self,
        _args: agent_client_protocol::AuthenticateRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn session_cancel(
        &mut self,
        _args: agent_client_protocol::CancelNotification,
        _cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn new_session(
        &mut self,
        _args: agent_client_protocol::NewSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::NewSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn load_session(
        &mut self,
        _args: agent_client_protocol::LoadSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn prompt(
        &mut self,
        _args: agent_client_protocol::PromptRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::PromptResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn set_session_mode(
        &mut self,
        _args: agent_client_protocol::SetSessionModeRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use scp::{AcpAgentToClientMessages, JsonRpcConnectionExt};

    use super::*;

    #[tokio::test]
    async fn test_single_component_no_proxy_capability() {
        let local = tokio::task::LocalSet::new();

        local
            .run_until(async {
                // Create mock component that will capture the initialize request
                let (mock, captured_init) = capturing_mock_component();

                // Create duplex streams for editor <-> conductor communication
                let (editor_out, conductor_in) = duplex(1024);
                let (conductor_out, editor_in) = duplex(1024);

                // Spawn conductor in a local task
                let conductor_handle = tokio::task::spawn_local(async move {
                    Conductor::run(
                        conductor_out.compat_write(),
                        conductor_in.compat(),
                        vec![ComponentProvider::Mock(Box::new(mock))],
                    )
                    .await
                });

                // Create editor-side JSON-RPC connection
                let editor_task = tokio::task::spawn_local(async move {
                    JsonRpcConnection::new(editor_out.compat_write(), editor_in.compat())
                        .with_client(async move |client| {
                            // Send initialize request as the editor
                            let init_request = InitializeRequest {
                                protocol_version: Default::default(),
                                client_capabilities: Default::default(),
                                meta: None,
                            };

                            let response = client
                                .send_json_request(
                                    "initialize".to_string(),
                                    serde_json::to_value(init_request).unwrap(),
                                )
                                .recv()
                                .await;

                            // Should get a successful response
                            assert!(
                                response.is_ok(),
                                "Initialize request should succeed: {:?}",
                                response
                            );

                            Ok::<(), jsonrpcmsg::Error>(())
                        })
                        .await
                });

                // Wait for the editor side to complete
                let _ = editor_task.await.expect("Editor task should complete");

                // Check what the component received
                let received = captured_init.lock().await;
                assert!(
                    received.is_some(),
                    "Component should have received initialize request"
                );

                let init_req = received.as_ref().unwrap();

                // Verify proxy capability is NOT present (single component chain)
                if let Some(meta) = &init_req.meta {
                    if let Some(symposium) = meta.get("symposium") {
                        assert!(
                            symposium.get("proxy").is_none()
                                || symposium.get("proxy") == Some(&serde_json::Value::Bool(false)),
                            "Single component should not have proxy capability"
                        );
                    }
                }

                // Clean up - conductor task will run until editor closes connection
                conductor_handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn test_two_component_proxy_chain() {
        crate::test_util::init_test_tracing();

        use agent_client_protocol::{ContentBlock, PromptRequest, SessionId, TextContent};

        let local = tokio::task::LocalSet::new();

        local
            .run_until(async {
                // Shared state for capturing what each component receives
                let component1_init = Arc::new(Mutex::new(None));
                let component2_init = Arc::new(Mutex::new(None));
                let component1_prompt = Arc::new(Mutex::new(None));
                let component2_prompt = Arc::new(Mutex::new(None));

                // Component 1: Forwards prompts with additions
                let c1_init = component1_init.clone();
                let c1_prompt = component1_prompt.clone();
                let component1 = MockComponentImpl::new(move |connection| async move {
                    let callbacks = Component1Callbacks {
                        captured_init: c1_init,
                        captured_prompt: c1_prompt,
                    };

                    let _ = connection
                        .on_receive(AcpClientToAgentMessages::callback(callbacks.clone()))
                        .on_receive_from_successor(AcpAgentToClientMessages::callback(callbacks))
                        .serve()
                        .instrument(tracing::info_span!("actor", id = "C1"))
                        .await;
                });

                // Component 2: Responds with "OK"
                let c2_init = component2_init.clone();
                let c2_prompt = component2_prompt.clone();
                let component2 = MockComponentImpl::new(move |connection| async move {
                    let c2_init = c2_init.clone();
                    let c2_prompt = c2_prompt.clone();

                    let _ = connection
                        .on_receive(AcpClientToAgentMessages::callback(Component2Callbacks {
                            captured_init: c2_init,
                            captured_prompt: c2_prompt,
                        }))
                        .serve()
                        .instrument(tracing::info_span!("actor", id = "C2"))
                        .await;
                });

                // Create duplex streams for editor <-> conductor communication
                let (editor_out, conductor_in) = duplex(1024);
                let (conductor_out, editor_in) = duplex(1024);

                // Spawn conductor with two components
                let conductor_handle = tokio::task::spawn_local(async move {
                    Conductor::run(
                        conductor_out.compat_write(),
                        conductor_in.compat(),
                        vec![
                            ComponentProvider::Mock(Box::new(component1)),
                            ComponentProvider::Mock(Box::new(component2)),
                        ],
                    )
                    .instrument(tracing::info_span!("actor", id = "conductor"))
                    .await
                });

                // Editor-side test
                let editor_collected_updates = Arc::new(std::sync::Mutex::new(String::new()));
                let editor_collected_updates_clone = editor_collected_updates.clone();
                let editor_task = tokio::task::spawn_local(async move {
                    JsonRpcConnection::new(editor_out.compat_write(), editor_in.compat())
                        .on_receive(AcpAgentToClientMessages::callback(EditorCallbacks {
                            collected_updates: editor_collected_updates_clone,
                        }))
                        .with_client(async move |client| {
                            // 1. Initialize
                            let init_request = InitializeRequest {
                                protocol_version: Default::default(),
                                client_capabilities: Default::default(),
                                meta: None,
                            };

                            let init_response = client
                                .send_json_request(
                                    "initialize".to_string(),
                                    serde_json::to_value(&init_request).unwrap(),
                                )
                                .recv()
                                .await;

                            assert!(
                                init_response.is_ok(),
                                "Initialize should succeed: {:?}",
                                init_response
                            );

                            // 2. Send a prompt
                            let prompt_request = PromptRequest {
                                session_id: SessionId("test-session".into()),
                                prompt: vec![ContentBlock::Text(TextContent {
                                    text: "User input".to_string(),
                                    annotations: None,
                                    meta: None,
                                })],
                                meta: None,
                            };

                            let prompt_response = client
                                .send_json_request(
                                    "session/prompt".to_string(),
                                    serde_json::to_value(&prompt_request).unwrap(),
                                )
                                .recv()
                                .await;

                            assert!(
                                prompt_response.is_ok(),
                                "Prompt should succeed: {:?}",
                                prompt_response
                            );

                            Ok::<(), jsonrpcmsg::Error>(())
                        })
                        .instrument(tracing::info_span!("actor", id = "Editor"))
                        .await
                });

                // Wait for editor to complete
                let _ = editor_task.await.expect("Editor task should complete");

                // Verify initialization
                let c1_init_req = component1_init.lock().await;
                assert!(
                    c1_init_req.is_some(),
                    "Component 1 should receive initialize"
                );
                if let Some(meta) = &c1_init_req.as_ref().unwrap().meta {
                    if let Some(symposium) = meta.get("symposium") {
                        assert_eq!(
                            symposium.get("proxy"),
                            Some(&serde_json::Value::Bool(true)),
                            "Component 1 should have proxy: true"
                        );
                    }
                }

                let c2_init_req = component2_init.lock().await;
                assert!(
                    c2_init_req.is_some(),
                    "Component 2 should receive initialize"
                );
                if let Some(meta) = &c2_init_req.as_ref().unwrap().meta {
                    if let Some(symposium) = meta.get("symposium") {
                        let proxy_value = symposium.get("proxy");
                        assert!(
                            proxy_value.is_none()
                                || proxy_value == Some(&serde_json::Value::Bool(false)),
                            "Component 2 should not have proxy capability"
                        );
                    }
                }

                // Verify prompts were forwarded
                let c1_prompt_req = component1_prompt.lock().await;
                assert!(c1_prompt_req.is_some(), "Component 1 should receive prompt");

                // Check component 1 received original text
                if let Some(ContentBlock::Text(text)) =
                    c1_prompt_req.as_ref().unwrap().prompt.first()
                {
                    assert_eq!(
                        text.text, "User input",
                        "Component 1 receives original prompt"
                    );
                } else {
                    panic!("Component 1 should receive text content");
                }

                let c2_prompt_req = component2_prompt.lock().await;
                assert!(c2_prompt_req.is_some(), "Component 2 should receive prompt");

                // Check component 2 received modified text
                if let Some(ContentBlock::Text(text)) =
                    c2_prompt_req.as_ref().unwrap().prompt.first()
                {
                    assert_eq!(
                        text.text, "User input + C1",
                        "Component 2 receives modified prompt from C1"
                    );
                } else {
                    panic!("Component 2 should receive text content");
                }

                // Verify editor received the session update modified by C1
                // C2 sends "OK from C2", C1 intercepts and modifies to "OK from C2 + C1"
                let collected = editor_collected_updates.lock().unwrap();
                assert_eq!(
                    *collected, "OK from C2 + C1",
                    "Editor should receive the session update modified by C1"
                );

                conductor_handle.abort();
            })
            .await;
    }
}

/// Callbacks for Component 1 (proxy component that forwards)
#[derive(Clone, Debug)]
struct Component1Callbacks {
    captured_init: Arc<Mutex<Option<InitializeRequest>>>,
    captured_prompt: Arc<Mutex<Option<agent_client_protocol::PromptRequest>>>,
}

impl AcpClientToAgentCallbacks for Component1Callbacks {
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: scp::JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        *self.captured_init.lock().await = Some(args.clone());

        let successor_response = response.send_request_to_successor(args);

        let current_span = tracing::Span::current();
        tokio::task::spawn_local(
            async move {
                let r = successor_response.recv().await;
                let _ = response.respond_with_result(r);
            }
            .instrument(current_span),
        );

        Ok(())
    }

    async fn prompt(
        &mut self,
        args: agent_client_protocol::PromptRequest,
        response: scp::JsonRpcRequestCx<agent_client_protocol::PromptResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        *self.captured_prompt.lock().await = Some(args.clone());

        // Forward to successor with modification - append " + C1" to text
        let mut modified_prompt = args.clone();
        if let Some(ContentBlock::Text(text)) = modified_prompt.prompt.first() {
            let mut modified_text = text.clone();
            modified_text.text = format!("{} + C1", text.text);
            modified_prompt.prompt = vec![ContentBlock::Text(modified_text)];
        }

        let successor_response = response
            .json_rpc_cx()
            .send_request_to_successor(modified_prompt);

        let current_span = tracing::Span::current();
        tokio::task::spawn_local(
            async move {
                let prompt_response = successor_response.recv().await;
                let _ = response.respond_with_result(prompt_response);
            }
            .instrument(current_span),
        );

        Ok(())
    }

    async fn authenticate(
        &mut self,
        _args: agent_client_protocol::AuthenticateRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn session_cancel(
        &mut self,
        _args: agent_client_protocol::CancelNotification,
        _cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn new_session(
        &mut self,
        _args: agent_client_protocol::NewSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::NewSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn load_session(
        &mut self,
        _args: agent_client_protocol::LoadSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn set_session_mode(
        &mut self,
        _args: agent_client_protocol::SetSessionModeRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }
}

impl AcpAgentToClientCallbacks for Component1Callbacks {
    async fn request_permission(
        &mut self,
        _args: agent_client_protocol::RequestPermissionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::RequestPermissionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn write_text_file(
        &mut self,
        _args: agent_client_protocol::WriteTextFileRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::WriteTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn read_text_file(
        &mut self,
        _args: agent_client_protocol::ReadTextFileRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::ReadTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn create_terminal(
        &mut self,
        _args: agent_client_protocol::CreateTerminalRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::CreateTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn terminal_output(
        &mut self,
        _args: agent_client_protocol::TerminalOutputRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::TerminalOutputResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn release_terminal(
        &mut self,
        _args: agent_client_protocol::ReleaseTerminalRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::ReleaseTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn wait_for_terminal_exit(
        &mut self,
        _args: agent_client_protocol::WaitForTerminalExitRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::WaitForTerminalExitResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn kill_terminal_command(
        &mut self,
        _args: agent_client_protocol::KillTerminalCommandRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::KillTerminalCommandResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn session_notification(
        &mut self,
        args: agent_client_protocol::SessionNotification,
        cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        use agent_client_protocol::{ContentBlock, SessionUpdate};

        // Modify the notification to show it passed through C1
        let mut modified_notification = args.clone();
        if let SessionUpdate::AgentMessageChunk { content } = &modified_notification.update {
            if let ContentBlock::Text(text) = content {
                let mut modified_text = text.clone();
                modified_text.text = format!("{} + C1", text.text);
                modified_notification.update = SessionUpdate::AgentMessageChunk {
                    content: ContentBlock::Text(modified_text),
                };
            }
        }

        // Forward the notification from successor to our client
        cx.send_notification(
            agent_client_protocol::AgentNotification::SessionNotification(modified_notification),
        )
        .map_err(scp::util::jsonrpc_to_acp_error)
    }
}

/// Callbacks for Component 2 (final component that responds)
struct Component2Callbacks {
    captured_init: Arc<Mutex<Option<InitializeRequest>>>,
    captured_prompt: Arc<Mutex<Option<agent_client_protocol::PromptRequest>>>,
}

impl AcpClientToAgentCallbacks for Component2Callbacks {
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: scp::JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        *self.captured_init.lock().await = Some(args);

        let _ = response.respond(InitializeResponse {
            protocol_version: Default::default(),
            agent_capabilities: Default::default(),
            auth_methods: vec![],
            meta: None,
        });
        Ok(())
    }

    async fn prompt(
        &mut self,
        args: agent_client_protocol::PromptRequest,
        response: scp::JsonRpcRequestCx<agent_client_protocol::PromptResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        use agent_client_protocol::{
            AgentNotification, ContentBlock, SessionNotification, SessionUpdate, StopReason,
            TextContent,
        };

        *self.captured_prompt.lock().await = Some(args.clone());

        // Send an update
        let _ = response
            .json_rpc_cx()
            .send_notification(AgentNotification::SessionNotification(
                SessionNotification {
                    session_id: args.session_id.clone(),
                    update: SessionUpdate::AgentMessageChunk {
                        content: ContentBlock::Text(TextContent {
                            text: "OK from C2".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                    },
                    meta: None,
                },
            ));

        // Send response
        let _ = response.respond(agent_client_protocol::PromptResponse {
            stop_reason: StopReason::EndTurn,
            meta: None,
        });

        Ok(())
    }

    async fn authenticate(
        &mut self,
        _args: agent_client_protocol::AuthenticateRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn session_cancel(
        &mut self,
        _args: agent_client_protocol::CancelNotification,
        _cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn new_session(
        &mut self,
        _args: agent_client_protocol::NewSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::NewSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn load_session(
        &mut self,
        _args: agent_client_protocol::LoadSessionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn set_session_mode(
        &mut self,
        _args: agent_client_protocol::SetSessionModeRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }
}

/// Callbacks for the editor (receives notifications from components)
struct EditorCallbacks {
    collected_updates: Arc<std::sync::Mutex<String>>,
}

impl AcpAgentToClientCallbacks for EditorCallbacks {
    async fn session_notification(
        &mut self,
        args: agent_client_protocol::SessionNotification,
        _cx: &JsonRpcCx,
    ) -> Result<(), agent_client_protocol::Error> {
        use agent_client_protocol::{ContentBlock, SessionUpdate, TextContent};

        // Collect text content from session updates
        if let SessionUpdate::AgentMessageChunk { content } = &args.update {
            if let ContentBlock::Text(TextContent { text, .. }) = content {
                self.collected_updates
                    .lock()
                    .unwrap()
                    .push_str(text.as_str());
            }
        }
        Ok(())
    }

    async fn request_permission(
        &mut self,
        _args: agent_client_protocol::RequestPermissionRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::RequestPermissionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn write_text_file(
        &mut self,
        _args: agent_client_protocol::WriteTextFileRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::WriteTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn read_text_file(
        &mut self,
        _args: agent_client_protocol::ReadTextFileRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::ReadTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn create_terminal(
        &mut self,
        _args: agent_client_protocol::CreateTerminalRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::CreateTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn terminal_output(
        &mut self,
        _args: agent_client_protocol::TerminalOutputRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::TerminalOutputResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn release_terminal(
        &mut self,
        _args: agent_client_protocol::ReleaseTerminalRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::ReleaseTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn wait_for_terminal_exit(
        &mut self,
        _args: agent_client_protocol::WaitForTerminalExitRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::WaitForTerminalExitResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn kill_terminal_command(
        &mut self,
        _args: agent_client_protocol::KillTerminalCommandRequest,
        _response: scp::JsonRpcRequestCx<agent_client_protocol::KillTerminalCommandResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }
}

#[cfg(test)]
mod mcp_capability_tests {
    use super::*;
    use scp::{InitializeResponseExt, McpAcpTransport};

    /// Helper to create a mock component that responds with or without mcp_acp_transport capability
    fn mock_component_with_capability(
        has_capability: bool,
    ) -> (MockComponentImpl, Arc<Mutex<Option<InitializeResponse>>>) {
        let captured_response = Arc::new(Mutex::new(None));
        let captured_response_clone = captured_response.clone();

        let mock = MockComponentImpl::new(move |connection| {
            let captured = captured_response_clone.clone();
            async move {
                let _ = connection
                    .on_receive(AcpClientToAgentMessages::callback(
                        MockComponentWithCapability {
                            has_capability,
                            captured_response: captured,
                        },
                    ))
                    .serve()
                    .instrument(tracing::info_span!("actor", id = "mock_agent"))
                    .await;
            }
        });

        (mock, captured_response)
    }

    struct MockComponentWithCapability {
        has_capability: bool,
        captured_response: Arc<Mutex<Option<InitializeResponse>>>,
    }

    impl AcpClientToAgentCallbacks for MockComponentWithCapability {
        async fn initialize(
            &mut self,
            _args: InitializeRequest,
            response: scp::JsonRpcRequestCx<InitializeResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            let mut init_response = InitializeResponse {
                protocol_version: Default::default(),
                agent_capabilities: Default::default(),
                auth_methods: vec![],
                meta: None,
            };

            // Add capability if requested
            if self.has_capability {
                init_response = init_response.add_meta_capability(McpAcpTransport);
            }

            // Capture what we're sending for test verification
            *self.captured_response.lock().await = Some(init_response.clone());

            let _ = response.respond(init_response);
            Ok(())
        }

        async fn authenticate(
            &mut self,
            _args: agent_client_protocol::AuthenticateRequest,
            _response: scp::JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }

        async fn session_cancel(
            &mut self,
            _args: agent_client_protocol::CancelNotification,
            _cx: &JsonRpcCx,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }

        async fn new_session(
            &mut self,
            _args: agent_client_protocol::NewSessionRequest,
            _response: scp::JsonRpcRequestCx<agent_client_protocol::NewSessionResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }

        async fn load_session(
            &mut self,
            _args: agent_client_protocol::LoadSessionRequest,
            _response: scp::JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }

        async fn prompt(
            &mut self,
            _args: agent_client_protocol::PromptRequest,
            _response: scp::JsonRpcRequestCx<agent_client_protocol::PromptResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }

        async fn set_session_mode(
            &mut self,
            _args: agent_client_protocol::SetSessionModeRequest,
            _response: scp::JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
        ) -> Result<(), agent_client_protocol::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_agent_without_mcp_capability() {
        crate::test_util::init_test_tracing();

        let local = tokio::task::LocalSet::new();

        local
            .run_until(async {
                // Create mock agent that does NOT have mcp_acp_transport capability
                let (mock_agent, agent_response) = mock_component_with_capability(false);

                // Set up duplex streams for editor ↔ conductor communication
                let (editor_write, conductor_read) = duplex(1024);
                let (conductor_write, editor_read) = duplex(1024);

                // Start conductor with the mock agent
                let conductor_handle = tokio::task::spawn_local(async move {
                    Conductor::run(
                        conductor_write.compat_write(),
                        conductor_read.compat(),
                        vec![ComponentProvider::Mock(Box::new(mock_agent))],
                    )
                    .await
                });

                // Editor side - send initialize request
                let editor_task = tokio::task::spawn_local(async move {
                    JsonRpcConnection::new(editor_write.compat_write(), editor_read.compat())
                        .with_client(async move |client| {
                            let response = client
                                .send_request(agent_client_protocol::InitializeRequest {
                                    protocol_version: Default::default(),
                                    client_capabilities: Default::default(),
                                    meta: None,
                                })
                                .recv()
                                .await;

                            assert!(
                                response.is_ok(),
                                "Initialize should succeed: {:?}",
                                response
                            );

                            let init_response = response.unwrap();

                            // Verify the editor receives the response WITH the capability added
                            assert!(
                                init_response.has_meta_capability(McpAcpTransport),
                                "Editor should see mcp_acp_transport capability added by conductor"
                            );

                            Ok::<(), jsonrpcmsg::Error>(())
                        })
                        .await
                });

                let _ = editor_task.await.expect("Editor task should complete");

                // Verify the agent sent response WITHOUT the capability
                let agent_resp = agent_response.lock().await;
                assert!(agent_resp.is_some(), "Agent should have responded");
                let resp = agent_resp.as_ref().unwrap();
                assert!(
                    !resp.has_meta_capability(McpAcpTransport),
                    "Agent's original response should not have mcp_acp_transport"
                );

                conductor_handle.abort();
            })
            .await;
    }

    #[tokio::test]
    async fn test_agent_with_mcp_capability() {
        crate::test_util::init_test_tracing();

        let local = tokio::task::LocalSet::new();

        local
            .run_until(async {
                // Create mock agent that DOES have mcp_acp_transport capability
                let (mock_agent, agent_response) = mock_component_with_capability(true);

                let (editor_write, conductor_read) = duplex(1024);
                let (conductor_write, editor_read) = duplex(1024);

                let conductor_handle = tokio::task::spawn_local(async move {
                    Conductor::run(
                        conductor_write.compat_write(),
                        conductor_read.compat(),
                        vec![ComponentProvider::Mock(Box::new(mock_agent))],
                    )
                    .await
                });

                let editor_task = tokio::task::spawn_local(async move {
                    JsonRpcConnection::new(editor_write.compat_write(), editor_read.compat())
                        .with_client(async move |client| {
                            let response = client
                                .send_request(agent_client_protocol::InitializeRequest {
                                    protocol_version: Default::default(),
                                    client_capabilities: Default::default(),
                                    meta: None,
                                })
                                .recv()
                                .await;

                            assert!(
                                response.is_ok(),
                                "Initialize should succeed: {:?}",
                                response
                            );

                            let init_response = response.unwrap();

                            // Verify capability is still present
                            assert!(
                                init_response.has_meta_capability(McpAcpTransport),
                                "Editor should see mcp_acp_transport capability from agent"
                            );

                            Ok::<(), jsonrpcmsg::Error>(())
                        })
                        .await
                });

                let _ = editor_task.await.expect("Editor task should complete");

                // Verify the agent sent response WITH the capability
                let agent_resp = agent_response.lock().await;
                assert!(agent_resp.is_some(), "Agent should have responded");
                let resp = agent_resp.as_ref().unwrap();
                assert!(
                    resp.has_meta_capability(McpAcpTransport),
                    "Agent's response should have mcp_acp_transport"
                );

                conductor_handle.abort();
            })
            .await;
    }
}
