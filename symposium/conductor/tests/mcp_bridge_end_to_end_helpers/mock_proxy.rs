//! Mock proxy component that provides go_go_gadget_shoes MCP tool

use std::sync::Arc;

use agent_client_protocol::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse,
};
use conductor::component::MockComponentImpl;
use scp::{
    AcpClientToAgentCallbacks, AcpClientToAgentMessages, JsonRpcCx, JsonRpcCxExt, JsonRpcRequestCx,
};
use tokio::sync::Mutex;
use tracing::Instrument;

/// State shared between mock proxy callbacks
#[derive(Clone)]
struct ProxyState {
    /// UUID for the MCP server this proxy provides
    mcp_server_uuid: String,
    /// Counter for tool invocations (for test assertions)
    tool_invocation_count: Arc<Mutex<u32>>,
}

/// Callbacks for the mock proxy component
struct ProxyCallbacks {
    state: ProxyState,
}

impl AcpClientToAgentCallbacks for ProxyCallbacks {
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Proxy: received initialize");

        // Check if we have the proxy capability (should be present)
        let has_proxy_capability = args
            .meta
            .as_ref()
            .and_then(|m| m.get("symposium"))
            .and_then(|s| s.get("proxy"))
            .and_then(|p| p.as_bool())
            .unwrap_or(false);

        tracing::info!("Proxy: has_proxy_capability = {}", has_proxy_capability);

        if has_proxy_capability {
            // Forward initialize to successor
            tracing::info!("Proxy: forwarding initialize to successor");

            let successor_response = response.json_rpc_cx().send_request_to_successor(args);

            let current_span = tracing::Span::current();
            tokio::task::spawn_local(
                async move {
                    let result = successor_response.recv().await;

                    tracing::info!("Proxy: received response from successor");

                    // Add our mcp_acp_transport capability to the response
                    let modified_result = result.map(|mut successor_resp| {
                        let mut meta = successor_resp.meta.unwrap_or_else(|| serde_json::json!({}));

                        if let Some(obj) = meta.as_object_mut() {
                            obj.insert("mcp_acp_transport".to_string(), serde_json::json!(true));
                        }
                        successor_resp.meta = Some(meta);
                        successor_resp
                    });

                    let _ = response.respond_with_result(modified_result);
                }
                .instrument(current_span),
            );
        } else {
            // No proxy capability means we're not in a chain
            let _ = response.respond(InitializeResponse {
                protocol_version: Default::default(),
                agent_capabilities: Default::default(),
                auth_methods: vec![],
                meta: Some(serde_json::json!({
                    "mcp_acp_transport": true
                })),
            });
        }

        Ok(())
    }

    async fn authenticate(
        &mut self,
        _args: agent_client_protocol::AuthenticateRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::AuthenticateResponse>,
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
        args: NewSessionRequest,
        response: JsonRpcRequestCx<NewSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Proxy: received new_session");

        // TODO: Inject our MCP server into the tool list
        // For now, just forward unchanged - actual MCP server injection
        // requires constructing McpServer struct properly

        let successor_response = response.json_rpc_cx().send_request_to_successor(args);

        let current_span = tracing::Span::current();
        tokio::task::spawn_local(
            async move {
                let result = successor_response.recv().await;
                tracing::info!("Proxy: received new_session response from successor");
                let _ = response.respond_with_result(result);
            }
            .instrument(current_span),
        );

        Ok(())
    }

    async fn load_session(
        &mut self,
        _args: agent_client_protocol::LoadSessionRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::LoadSessionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }

    async fn prompt(
        &mut self,
        args: PromptRequest,
        response: JsonRpcRequestCx<PromptResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::info!("Proxy: received prompt");

        // Forward to successor
        let successor_response = response.json_rpc_cx().send_request_to_successor(args);

        let current_span = tracing::Span::current();
        tokio::task::spawn_local(
            async move {
                let result = successor_response.recv().await;
                tracing::info!("Proxy: received prompt response from successor");
                let _ = response.respond_with_result(result);
            }
            .instrument(current_span),
        );

        Ok(())
    }

    async fn set_session_mode(
        &mut self,
        _args: agent_client_protocol::SetSessionModeRequest,
        _response: JsonRpcRequestCx<agent_client_protocol::SetSessionModeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        Ok(())
    }
}

/// Create a mock proxy component that provides the go_go_gadget_shoes tool
pub fn create_mock_proxy() -> MockComponentImpl {
    let state = ProxyState {
        mcp_server_uuid: uuid::Uuid::new_v4().to_string(),
        tool_invocation_count: Arc::new(Mutex::new(0)),
    };

    MockComponentImpl::new(move |connection| async move {
        let callbacks = ProxyCallbacks {
            state: state.clone(),
        };

        let _ = connection
            .on_receive(AcpClientToAgentMessages::callback(callbacks))
            .serve()
            .instrument(tracing::info_span!("actor", id = "mock_proxy"))
            .await;
    })
}
