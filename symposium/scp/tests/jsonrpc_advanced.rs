//! Advanced feature tests for JSON-RPC layer
//!
//! Tests advanced JSON-RPC capabilities:
//! - Bidirectional communication (both sides can be client+server)
//! - Request ID tracking and matching
//! - Out-of-order response handling

use futures::{AsyncRead, AsyncWrite};
use scp::{
    Handled, JsonRpcConnection, JsonRpcHandler, JsonRpcIncomingMessage, JsonRpcMessage,
    JsonRpcOutgoingMessage, JsonRpcRequest, JsonRpcRequestCx, JsonRpcResponse,
};
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test helper to block and wait for a JSON-RPC response.
async fn recv<R: JsonRpcIncomingMessage + Send>(
    response: JsonRpcResponse<R>,
) -> Result<R, agent_client_protocol::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    response.await_when_response_received(async move |result| {
        tx.send(result)
            .map_err(|_| agent_client_protocol::Error::internal_error())
    })?;
    rx.await
        .map_err(|_| agent_client_protocol::Error::internal_error())?
}

/// Helper to set up a client-server pair for testing.
fn setup_test_connections(
    server_handler: impl JsonRpcHandler + 'static,
) -> (
    JsonRpcConnection<impl AsyncWrite, impl AsyncRead, impl JsonRpcHandler>,
    JsonRpcConnection<impl AsyncWrite, impl AsyncRead, impl JsonRpcHandler>,
) {
    let (client_writer, server_reader) = tokio::io::duplex(1024);
    let (server_writer, client_reader) = tokio::io::duplex(1024);

    let server_reader = server_reader.compat();
    let server_writer = server_writer.compat_write();
    let client_reader = client_reader.compat();
    let client_writer = client_writer.compat_write();

    let server = JsonRpcConnection::new(server_writer, server_reader).on_receive(server_handler);
    let client = JsonRpcConnection::new(client_writer, client_reader);

    (server, client)
}

// ============================================================================
// Test types
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PingRequest {
    value: u32,
}

impl JsonRpcMessage for PingRequest {}

impl JsonRpcOutgoingMessage for PingRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        Ok(scp::UntypedMessage::new(
            method,
            serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "ping"
    }
}

impl JsonRpcRequest for PingRequest {
    type Response = PongResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct PongResponse {
    value: u32,
}

impl JsonRpcMessage for PongResponse {}

impl JsonRpcIncomingMessage for PongResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        scp::util::json_cast(&value)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowRequest {
    delay_ms: u64,
    id: u32,
}

impl JsonRpcMessage for SlowRequest {}

impl JsonRpcOutgoingMessage for SlowRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        Ok(scp::UntypedMessage::new(
            method,
            serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "slow"
    }
}

impl JsonRpcRequest for SlowRequest {
    type Response = SlowResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowResponse {
    id: u32,
}

impl JsonRpcMessage for SlowResponse {}

impl JsonRpcIncomingMessage for SlowResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        scp::util::json_cast(&value)
    }
}

// ============================================================================
// Test 1: Bidirectional communication
// ============================================================================

struct PingHandler;

impl JsonRpcHandler for PingHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        if cx.method() == "ping" {
            let request: PingRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            cx.cast().respond(PongResponse {
                value: request.value + 1,
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_bidirectional_communication() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            // Set up two connections that are symmetric - both can send and receive
            let (side_a, side_b) = setup_test_connections(PingHandler);

            // Spawn side_a as server
            tokio::task::spawn_local(async move {
                side_a.serve().await.ok();
            });

            // Use side_b as client
            let result = side_b
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    let request = PingRequest { value: 10 };
                    let response_future = recv(cx.send_request(request));
                    let response: Result<PongResponse, _> = response_future.await;

                    assert!(response.is_ok());
                    if let Ok(resp) = response {
                        assert_eq!(resp.value, 11);
                    }
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 2: Request IDs are properly tracked
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_request_ids() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(PingHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    // Send multiple requests and verify responses match
                    let req1 = PingRequest { value: 1 };
                    let req2 = PingRequest { value: 2 };
                    let req3 = PingRequest { value: 3 };

                    let resp1_future = recv(cx.send_request(req1));
                    let resp2_future = recv(cx.send_request(req2));
                    let resp3_future = recv(cx.send_request(req3));

                    let resp1: Result<PongResponse, _> = resp1_future.await;
                    let resp2: Result<PongResponse, _> = resp2_future.await;
                    let resp3: Result<PongResponse, _> = resp3_future.await;

                    // Verify each response corresponds to its request
                    assert_eq!(resp1.unwrap().value, 2); // 1 + 1
                    assert_eq!(resp2.unwrap().value, 3); // 2 + 1
                    assert_eq!(resp3.unwrap().value, 4); // 3 + 1

                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 3: Out-of-order responses
// ============================================================================

struct SlowHandler;

impl JsonRpcHandler for SlowHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        if cx.method() == "slow" {
            let request: SlowRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            // Simulate delay
            tokio::time::sleep(tokio::time::Duration::from_millis(request.delay_ms)).await;

            cx.cast().respond(SlowResponse { id: request.id })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_out_of_order_responses() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(SlowHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    // Send requests with different delays
                    // Request 1: 100ms delay
                    // Request 2: 50ms delay
                    // Request 3: 10ms delay
                    // Responses should arrive in order: 3, 2, 1

                    let req1 = SlowRequest {
                        delay_ms: 100,
                        id: 1,
                    };
                    let req2 = SlowRequest {
                        delay_ms: 50,
                        id: 2,
                    };
                    let req3 = SlowRequest {
                        delay_ms: 10,
                        id: 3,
                    };

                    let resp1_future = recv(cx.send_request(req1));
                    let resp2_future = recv(cx.send_request(req2));
                    let resp3_future = recv(cx.send_request(req3));

                    // Wait for all responses
                    let resp1: Result<SlowResponse, _> = resp1_future.await;
                    let resp2: Result<SlowResponse, _> = resp2_future.await;
                    let resp3: Result<SlowResponse, _> = resp3_future.await;

                    // Verify each future got the correct response despite out-of-order arrival
                    assert_eq!(resp1.unwrap().id, 1);
                    assert_eq!(resp2.unwrap().id, 2);
                    assert_eq!(resp3.unwrap().id, 3);

                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}
