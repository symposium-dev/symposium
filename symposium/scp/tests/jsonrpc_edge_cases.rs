//! Edge case tests for JSON-RPC layer
//!
//! Tests various edge cases and boundary conditions:
//! - Empty requests
//! - Null parameters
//! - Server shutdown scenarios
//! - Client disconnect handling

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

#[derive(Debug, Serialize, Deserialize)]
struct EmptyRequest;

impl JsonRpcRequest for EmptyRequest {
    type Response = SimpleResponse;

    fn method(&self) -> &str {
        "empty_method"
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OptionalParamsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

impl JsonRpcRequest for OptionalParamsRequest {
    type Response = SimpleResponse;

    fn method(&self) -> &str {
        "optional_params_method"
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SimpleResponse {
    result: String,
}

// ============================================================================
// Test 1: Empty request (no parameters)
// ============================================================================

struct EmptyParamsHandler;

impl JsonRpcHandler for EmptyParamsHandler {
    async fn handle_request(
        &mut self,
        method: &str,
        _params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method == "empty_method" {
            // Accept request with no params
            response.cast::<SimpleResponse>().respond(SimpleResponse {
                result: "Got empty request".to_string(),
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_empty_request() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(EmptyParamsHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
                    let request = EmptyRequest;

                    let result: Result<SimpleResponse, _> = cx.send_request(request).recv().await;

                    // Should succeed
                    assert!(result.is_ok());
                    if let Ok(response) = result {
                        assert_eq!(response.result, "Got empty request");
                    }
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 2: Null parameters
// ============================================================================

struct NullParamsHandler;

impl JsonRpcHandler for NullParamsHandler {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method == "optional_params_method" {
            // Check if params is None or contains null
            let has_params = params.is_some();

            response.cast::<SimpleResponse>().respond(SimpleResponse {
                result: format!("Has params: {}", has_params),
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_null_parameters() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(NullParamsHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
                    let request = OptionalParamsRequest { value: None };

                    let result: Result<SimpleResponse, _> = cx.send_request(request).recv().await;

                    // Should succeed - handler should handle null/missing params
                    assert!(result.is_ok());
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 3: Server shutdown with pending requests
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_server_shutdown() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(EmptyParamsHandler);

            let server_handle = tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let client_result = tokio::task::spawn_local(async move {
                client
                    .with_client(async |cx| -> Result<(), jsonrpcmsg::Error> {
                        let request = EmptyRequest;

                        // Send request and get future for response
                        let response_future = cx.send_request(request).recv();

                        // Give the request time to be sent over the wire
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                        // Try to get response (server should still be running briefly)
                        let _result: Result<SimpleResponse, _> = response_future.await;

                        // Could succeed or fail depending on timing
                        // The important thing is that it doesn't hang
                        Ok(())
                    })
                    .await
            });

            // Let the client send its request
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

            // Abort the server
            server_handle.abort();

            // Wait for client to finish
            let result = client_result.await;
            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 4: Client disconnect mid-request
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_client_disconnect() {
    use tokio::io::AsyncWriteExt;
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (mut client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, _client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();

            let server =
                JsonRpcConnection::new(server_writer, server_reader).on_receive(EmptyParamsHandler);

            tokio::task::spawn_local(async move {
                let _ = server.serve().await;
            });

            // Send partial request and then disconnect
            let partial_request = b"{\"jsonrpc\":\"2.0\",\"method\":\"empty_method\",\"id\":1";
            client_writer.write_all(partial_request).await.unwrap();
            client_writer.flush().await.unwrap();

            // Drop the writer to disconnect
            drop(client_writer);

            // Give server time to process the disconnect
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Server should handle this gracefully and terminate
            // (We can't really assert much here except that the test completes)
        })
        .await;
}
