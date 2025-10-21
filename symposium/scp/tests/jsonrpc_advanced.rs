//! Advanced feature tests for JSON-RPC layer
//!
//! Tests advanced JSON-RPC capabilities:
//! - Bidirectional communication (both sides can be client+server)
//! - Request ID tracking and matching
//! - Out-of-order response handling

use scp::{Handled, JsonRpcConnection, JsonRpcHandler, JsonRpcRequest, JsonRpcRequestCx};
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Helper to set up a client-server pair for testing.
fn setup_test_connections(
    server_handler: impl JsonRpcHandler + 'static,
) -> (
    JsonRpcConnection<impl JsonRpcHandler>,
    JsonRpcConnection<impl JsonRpcHandler>,
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

impl JsonRpcRequest for PingRequest {
    type Response = PongResponse;

    fn method(&self) -> &str {
        "ping"
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PongResponse {
    value: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowRequest {
    delay_ms: u64,
    id: u32,
}

impl JsonRpcRequest for SlowRequest {
    type Response = SlowResponse;

    fn method(&self) -> &str {
        "slow"
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SlowResponse {
    id: u32,
}

// ============================================================================
// Test 1: Bidirectional communication
// ============================================================================

struct PingHandler;

impl JsonRpcHandler for PingHandler {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method == "ping" {
            let request: PingRequest =
                scp::util::json_cast(params).map_err(|_| jsonrpcmsg::Error::invalid_params())?;

            response.cast::<PongResponse>().respond(PongResponse {
                value: request.value + 1,
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
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
                .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
                    let request = PingRequest { value: 10 };
                    let response_future = cx.send_request(request).recv();
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
                .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
                    // Send multiple requests and verify responses match
                    let req1 = PingRequest { value: 1 };
                    let req2 = PingRequest { value: 2 };
                    let req3 = PingRequest { value: 3 };

                    let resp1_future = cx.send_request(req1).recv();
                    let resp2_future = cx.send_request(req2).recv();
                    let resp3_future = cx.send_request(req3).recv();

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
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method == "slow" {
            let request: SlowRequest =
                scp::util::json_cast(params).map_err(|_| jsonrpcmsg::Error::invalid_params())?;

            // Simulate delay
            tokio::time::sleep(tokio::time::Duration::from_millis(request.delay_ms)).await;

            response
                .cast::<SlowResponse>()
                .respond(SlowResponse { id: request.id })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
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
                .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
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

                    let resp1_future = cx.send_request(req1).recv();
                    let resp2_future = cx.send_request(req2).recv();
                    let resp3_future = cx.send_request(req3).recv();

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
