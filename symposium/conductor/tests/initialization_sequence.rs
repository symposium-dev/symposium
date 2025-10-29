//! Integration tests for the initialization sequence and proxy capability handshake.
//!
//! These tests verify that:
//! 1. Single-component chains do NOT receive the proxy capability offer
//! 2. Multi-component chains: first component(s) receive proxy capability offer
//! 3. Proxy components must accept the capability or initialization fails
//! 4. Last component (agent) never receives proxy capability offer

use acp_proxy::JsonRpcCxExt;
use agent_client_protocol::{self as acp, AgentCapabilities};
use agent_client_protocol::{InitializeRequest, InitializeResponse};
use conductor::component::{Cleanup, ComponentProvider};
use conductor::conductor::Conductor;
use futures::{AsyncRead, AsyncWrite};
use scp::{JsonRpcConnection, JsonRpcConnectionCx, MetaCapabilityExt, Proxy};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::io::duplex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test helper to receive a JSON-RPC response
async fn recv<R: scp::JsonRpcResponsePayload + Send>(
    response: scp::JsonRpcResponse<R>,
) -> Result<R, agent_client_protocol::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    response.await_when_result_received(async move |result| {
        tx.send(result)
            .map_err(|_| agent_client_protocol::Error::internal_error())
    })?;
    rx.await
        .map_err(|_| agent_client_protocol::Error::internal_error())?
}

struct InitConfig {
    respond_with_proxy: bool,
    /// If true, forward the request WITH the proxy capability still attached (error case)
    forward_with_proxy: bool,
    offered_proxy: Mutex<Option<bool>>,
}

impl InitConfig {
    fn new(respond_with_proxy: bool) -> Arc<Self> {
        Arc::new(Self {
            respond_with_proxy,
            forward_with_proxy: false,
            offered_proxy: Mutex::new(None),
        })
    }

    fn new_with_forward_behavior(respond_with_proxy: bool, forward_with_proxy: bool) -> Arc<Self> {
        Arc::new(Self {
            respond_with_proxy,
            forward_with_proxy,
            offered_proxy: Mutex::new(None),
        })
    }

    fn read_offered_proxy(&self) -> Option<bool> {
        *self.offered_proxy.lock().expect("not poisoned")
    }
}

struct InitComponentProvider {
    config: Arc<InitConfig>,
}

impl InitComponentProvider {
    fn new(config: &Arc<InitConfig>) -> Box<dyn ComponentProvider> {
        Box::new(Self {
            config: config.clone(),
        })
    }
}

impl ComponentProvider for InitComponentProvider {
    fn create(
        &self,
        cx: &JsonRpcConnectionCx,
        outgoing_bytes: Pin<Box<dyn AsyncWrite + Send>>,
        incoming_bytes: Pin<Box<dyn AsyncRead + Send>>,
    ) -> Result<Cleanup, acp::Error> {
        let config = Arc::clone(&self.config);
        cx.spawn(async move {
            JsonRpcConnection::new(outgoing_bytes, incoming_bytes)
                .name("init-component-provider")
                .on_receive_request(async move |mut request: InitializeRequest, request_cx| {
                    let has_proxy_capability = request.has_meta_capability(Proxy);
                    *config.offered_proxy.lock().expect("unpoisoned") = Some(has_proxy_capability);

                    // Conditionally remove proxy capability based on config
                    if !config.forward_with_proxy {
                        request = request.remove_meta_capability(Proxy);
                    }

                    if config.respond_with_proxy {
                        request_cx
                            .send_request_to_successor(request)
                            .await_when_result_received(async move |response| {
                                let mut response = response?;
                                assert!(!response.has_meta_capability(Proxy));
                                response = response.add_meta_capability(Proxy);
                                request_cx.respond(response)
                            })
                    } else {
                        let response = InitializeResponse {
                            protocol_version: request.protocol_version,
                            agent_capabilities: AgentCapabilities::default(),
                            auth_methods: vec![],
                            meta: None,
                            agent_info: None,
                        };

                        request_cx.respond(response)
                    }
                })
                .serve()
                .await
        })?;

        Ok(Cleanup::None)
    }
}

async fn run_test_with_components(
    components: Vec<Box<dyn ComponentProvider>>,
    editor_task: impl AsyncFnOnce(JsonRpcConnectionCx) -> Result<(), acp::Error>,
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
async fn test_single_component_no_proxy_offer() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    // Create a single mock component
    let component1 = InitConfig::new(false);

    run_test_with_components(
        vec![InitComponentProvider::new(&component1)],
        async |editor_cx| {
            let init_response = recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
                client_info: None,
            }))
            .await;

            assert!(
                init_response.is_ok(),
                "Initialize should succeed: {:?}",
                init_response
            );

            Ok::<(), agent_client_protocol::Error>(())
        },
    )
    .await?;

    assert_eq!(component1.read_offered_proxy(), Some(false));

    Ok(())
}

#[tokio::test]
async fn test_two_components() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    // Create a single mock component
    let component1 = InitConfig::new(true);
    let component2 = InitConfig::new(false);

    run_test_with_components(
        vec![
            InitComponentProvider::new(&component1),
            InitComponentProvider::new(&component2),
        ],
        async |editor_cx| {
            let init_response = recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
                client_info: None,
            }))
            .await;

            assert!(
                init_response.is_ok(),
                "Initialize should succeed: {:?}",
                init_response
            );

            Ok::<(), agent_client_protocol::Error>(())
        },
    )
    .await?;

    assert_eq!(component1.read_offered_proxy(), Some(true));
    assert_eq!(component2.read_offered_proxy(), Some(false));

    Ok(())
}

#[tokio::test]
async fn test_proxy_component_must_respond_with_proxy() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    // Component is offered proxy but does NOT respond with it (respond_with_proxy: false)
    let component1 = InitConfig::new(false);
    let component2 = InitConfig::new(false);

    let result = run_test_with_components(
        vec![
            InitComponentProvider::new(&component1),
            InitComponentProvider::new(&component2),
        ],
        async |editor_cx| {
            let init_response = recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
                client_info: None,
            }))
            .await;

            // Should fail because component1 was offered proxy but didn't respond with it
            assert!(
                init_response.is_err(),
                "Initialize should fail when proxy component doesn't respond with proxy capability"
            );

            Ok::<(), agent_client_protocol::Error>(())
        },
    )
    .await;

    // Verify the error occurred
    assert!(result.is_err(), "Expected conductor to return an error");
    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("component 0 is not a proxy"),
        "Expected 'component 0 is not a proxy' error, got: {:?}",
        error
    );

    // Verify component1 was offered proxy
    assert_eq!(component1.read_offered_proxy(), Some(true));

    Ok(())
}

#[tokio::test]
async fn test_proxy_component_must_strip_proxy_when_forwarding() -> Result<(), acp::Error> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("conductor=debug".parse().unwrap()),
        )
        .with_test_writer()
        .try_init();

    // Component responds with proxy BUT incorrectly forwards the request with proxy still attached
    let component1 = InitConfig::new_with_forward_behavior(true, true);
    let component2 = InitConfig::new(false);

    let result = run_test_with_components(
        vec![
            InitComponentProvider::new(&component1),
            InitComponentProvider::new(&component2),
        ],
        async |editor_cx| {
            let init_response = recv(editor_cx.send_request(InitializeRequest {
                protocol_version: Default::default(),
                client_capabilities: Default::default(),
                meta: None,
                client_info: None,
            }))
            .await;

            // Should fail because component1 forwarded request with proxy capability still attached
            assert!(
                init_response.is_err(),
                "Initialize should fail when proxy component forwards request with proxy capability"
            );

            Ok::<(), agent_client_protocol::Error>(())
        },
    )
    .await;

    // Verify the error occurred
    assert!(result.is_err(), "Expected conductor to return an error");
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conductor received unexpected initialization request with proxy capability"),
        "Expected 'conductor received unexpected initialization request with proxy capability' error, got: {:?}",
        error
    );

    // Verify component1 was offered proxy
    assert_eq!(component1.read_offered_proxy(), Some(true));

    Ok(())
}
