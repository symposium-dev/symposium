//! Edge case tests for JSON-RPC layer
//!
//! Tests various edge cases and boundary conditions:
//! - Empty requests
//! - Null parameters
//! - Server shutdown scenarios
//! - Client disconnect handling

use futures::{AsyncRead, AsyncWrite};
use scp::{
    JsonRpcConnection, JsonRpcMessage, JsonRpcRequest, JsonRpcRequestCx, JsonRpcResponse,
    JsonRpcResponsePayload,
};
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test helper to block and wait for a JSON-RPC response.
async fn recv<R: JsonRpcResponsePayload + Send>(
    response: JsonRpcResponse<R>,
) -> Result<R, agent_client_protocol::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    response.await_when_result_received(async move |result| {
        tx.send(result)
            .map_err(|_| agent_client_protocol::Error::internal_error())
    })?;
    rx.await
        .map_err(|_| agent_client_protocol::Error::internal_error())?
}

/// Helper to set up test streams.
fn setup_test_streams() -> (
    impl AsyncRead,
    impl AsyncWrite,
    impl AsyncRead,
    impl AsyncWrite,
) {
    let (client_writer, server_reader) = tokio::io::duplex(1024);
    let (server_writer, client_reader) = tokio::io::duplex(1024);

    let server_reader = server_reader.compat();
    let server_writer = server_writer.compat_write();
    let client_reader = client_reader.compat();
    let client_writer = client_writer.compat_write();

    (server_reader, server_writer, client_reader, client_writer)
}

// ============================================================================
// Test types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct EmptyRequest;

impl JsonRpcMessage for EmptyRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "empty_method"
    }

    fn parse_request(
        method: &str,
        _params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "empty_method" {
            return None;
        }
        Some(Ok(EmptyRequest))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for EmptyRequest {
    type Response = SimpleResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct OptionalParamsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

impl JsonRpcMessage for OptionalParamsRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "optional_params_method"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "optional_params_method" {
            return None;
        }
        Some(scp::util::json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for OptionalParamsRequest {
    type Response = SimpleResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct SimpleResponse {
    result: String,
}

impl JsonRpcResponsePayload for SimpleResponse {
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
// Test 1: Empty request (no parameters)
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_empty_request() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server_reader, server_writer, client_reader, client_writer) = setup_test_streams();

            let server = JsonRpcConnection::new(server_writer, server_reader).on_receive_request(
                async |_request: EmptyRequest, request_cx: JsonRpcRequestCx<SimpleResponse>| {
                    request_cx.respond(SimpleResponse {
                        result: "Got empty request".to_string(),
                    })
                },
            );

            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    let request = EmptyRequest;

                    let result: Result<SimpleResponse, _> = recv(cx.send_request(request)).await;

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

#[tokio::test(flavor = "current_thread")]
async fn test_null_params() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server_reader, server_writer, client_reader, client_writer) = setup_test_streams();

            let server = JsonRpcConnection::new(server_writer, server_reader).on_receive_request(
                async |_request: OptionalParamsRequest,
                       request_cx: JsonRpcRequestCx<SimpleResponse>| {
                    request_cx.respond(SimpleResponse {
                        result: "Has params: true".to_string(),
                    })
                },
            );

            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    let request = OptionalParamsRequest { value: None };

                    let result: Result<SimpleResponse, _> = recv(cx.send_request(request)).await;

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
            let (server_reader, server_writer, client_reader, client_writer) = setup_test_streams();

            let server = JsonRpcConnection::new(server_writer, server_reader).on_receive_request(
                async |_request: EmptyRequest, request_cx: JsonRpcRequestCx<SimpleResponse>| {
                    request_cx.respond(SimpleResponse {
                        result: "Got empty request".to_string(),
                    })
                },
            );

            let client = JsonRpcConnection::new(client_writer, client_reader);

            let server_handle = tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let client_result = tokio::task::spawn_local(async move {
                client
                    .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                        let request = EmptyRequest;

                        // Send request and get future for response
                        let response_future = recv(cx.send_request(request));

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

            let server = JsonRpcConnection::new(server_writer, server_reader).on_receive_request(
                async |_request: EmptyRequest, request_cx: JsonRpcRequestCx<SimpleResponse>| {
                    request_cx.respond(SimpleResponse {
                        result: "Got empty request".to_string(),
                    })
                },
            );

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
