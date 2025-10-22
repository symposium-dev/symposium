use std::{future::Future, pin::Pin, sync::Arc};

use agent_client_protocol::{InitializeRequest, InitializeResponse};
use futures::{AsyncRead, AsyncWrite};
use scp::{AcpClientToAgentCallbacks, AcpClientToAgentMessages, JsonRpcConnection, JsonRpcCx};
use tokio::{io::duplex, sync::Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{
    component::{ComponentProvider, MockComponent},
    conductor::Conductor,
};

/// A mock component that captures initialize requests for test verification.
struct CapturingMockComponent {
    captured_init: Arc<Mutex<Option<InitializeRequest>>>,
}

impl CapturingMockComponent {
    fn new() -> (Self, Arc<Mutex<Option<InitializeRequest>>>) {
        let captured = Arc::new(Mutex::new(None));
        (
            Self {
                captured_init: captured.clone(),
            },
            captured,
        )
    }
}

impl MockComponent for CapturingMockComponent {
    fn create(
        &self,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = anyhow::Result<(
                        Pin<Box<dyn AsyncWrite + Send>>,
                        Pin<Box<dyn AsyncRead + Send>>,
                    )>,
                > + Send,
        >,
    > {
        let captured_init = self.captured_init.clone();
        Box::pin(async move {
            // Create two duplex pairs for bidirectional communication
            let (conductor_out, component_in) = duplex(1024);
            let (component_out, conductor_in) = duplex(1024);

            // Spawn local task to run the mock component's JSON-RPC handler
            tokio::task::spawn_local(async move {
                let _ = JsonRpcConnection::new(component_out.compat_write(), component_in.compat())
                    .on_receive(AcpClientToAgentMessages::callback(CapturingCallbacks {
                        captured_init,
                    }))
                    .serve()
                    .await;
            });

            // Return conductor's ends of the streams
            Ok((
                Box::pin(conductor_out.compat_write()) as Pin<Box<dyn AsyncWrite + Send>>,
                Box::pin(conductor_in.compat()) as Pin<Box<dyn AsyncRead + Send>>,
            ))
        })
    }
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
    use super::*;

    #[tokio::test]
    async fn test_single_component_no_proxy_capability() {
        let local = tokio::task::LocalSet::new();

        local
            .run_until(async {
                // Create mock component that will capture the initialize request
                let (mock, captured_init) = CapturingMockComponent::new();

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
}
