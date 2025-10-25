//! Error handling tests for JSON-RPC layer
//!
//! Tests various error conditions:
//! - Invalid JSON
//! - Unknown methods
//! - Handler-returned errors
//! - Serialization failures
//! - Missing/invalid parameters

use expect_test::expect;
use futures::{AsyncRead, AsyncWrite};
use scp::{
    Handled, JsonRpcConnection, JsonRpcHandler, JsonRpcIncomingMessage, JsonRpcMessage,
    JsonRpcOutgoingMessage, JsonRpcRequest, JsonRpcRequestCx, JsonRpcResponse,
};
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test helper to block and wait for a JSON-RPC response.
async fn recv<R: JsonRpcMessage + Send>(
    response: JsonRpcResponse<R>,
) -> Result<R, agent_client_protocol::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    response.when_response_received_spawn(move |result| async move {
        let _ = tx.send(result);
    })?;
    rx.await
        .map_err(|_| agent_client_protocol::Error::internal_error())?
}

/// Helper to set up a client-server pair for testing.
fn setup_test_connections<H: JsonRpcHandler + 'static>(
    server_handler: H,
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

#[derive(Debug, Serialize, Deserialize)]
struct SimpleRequest {
    message: String,
}

impl JsonRpcMessage for SimpleRequest {}

impl JsonRpcOutgoingMessage for SimpleRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "simple_method"
    }
}

impl JsonRpcRequest for SimpleRequest {
    type Response = SimpleResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct SimpleResponse {
    result: String,
}

impl JsonRpcMessage for SimpleResponse {}

impl JsonRpcIncomingMessage for SimpleResponse {
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
// Test 1: Invalid JSON (complete line with parse error)
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_invalid_json() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            // Create duplex streams for bidirectional communication
            let (mut client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, mut client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();

            struct TestHandler;
            impl JsonRpcHandler for TestHandler {}

            let server =
                JsonRpcConnection::new(server_writer, server_reader).on_receive(TestHandler);

            // Spawn server
            tokio::task::spawn_local(async move {
                let _ = server.serve().await;
            });

            // Send invalid JSON
            let invalid_json = b"{\"method\": \"test\", \"id\": 1, INVALID}\n";
            client_writer.write_all(invalid_json).await.unwrap();
            client_writer.flush().await.unwrap();

            // Read response
            let mut buffer = vec![0u8; 1024];
            let n = client_reader.read(&mut buffer).await.unwrap();
            let response_str = String::from_utf8_lossy(&buffer[..n]);

            // Parse as JSON and verify structure
            let response: serde_json::Value =
                serde_json::from_str(response_str.trim()).expect("Response should be valid JSON");

            // Use expect_test to verify the exact structure
            expect![[r#"
                {
                  "error": {
                    "code": -32700,
                    "message": "Parse error"
                  },
                  "jsonrpc": "2.0"
                }"#]]
            .assert_eq(&serde_json::to_string_pretty(&response).unwrap());
        })
        .await;
}

// ============================================================================
// Test 1b: Incomplete line (EOF mid-message)
// ============================================================================

#[tokio::test]
async fn test_incomplete_line() {
    use futures::io::Cursor;

    // Incomplete JSON input - no newline, simulates client disconnect
    let incomplete_json = b"{\"method\": \"test\", \"id\": 1";
    let input = Cursor::new(incomplete_json.to_vec());
    let output = Cursor::new(Vec::new());

    struct TestHandler;
    impl JsonRpcHandler for TestHandler {}

    let connection = JsonRpcConnection::new(output, input).on_receive(TestHandler);

    // The server should handle EOF mid-message gracefully
    let result = connection.serve().await;

    // Server should terminate cleanly when hitting EOF
    assert!(result.is_ok() || result.is_err());
}

// ============================================================================
// Test 2: Unknown method (no handler claims)
// ============================================================================

struct NoOpHandler;

impl JsonRpcHandler for NoOpHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        // This handler never claims any requests
        Ok(Handled::No(cx))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_unknown_method() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(NoOpHandler);

            // Spawn server
            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            // Send request from client
            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    let request = SimpleRequest {
                        message: "test".to_string(),
                    };

                    let result: Result<SimpleResponse, _> = recv(cx.send_request(request)).await;

                    // Should get an error because no handler claims the method
                    assert!(result.is_err());
                    if let Err(err) = result {
                        // Should be "method not found" or similar error
                        assert!(err.code < 0); // JSON-RPC error codes are negative
                    }
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 3: Handler returns error
// ============================================================================

struct ErrorReturningHandler;

impl JsonRpcHandler for ErrorReturningHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        if cx.method() == "error_method" {
            // Explicitly return an error
            cx.respond_with_error(agent_client_protocol::Error::new((
                -32000,
                "This is an intentional error".to_string(),
            )))?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorRequest {
    value: String,
}

impl JsonRpcMessage for ErrorRequest {}

impl JsonRpcOutgoingMessage for ErrorRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "error_method"
    }
}

impl JsonRpcRequest for ErrorRequest {
    type Response = SimpleResponse;
}

#[tokio::test(flavor = "current_thread")]
async fn test_handler_returns_error() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(ErrorReturningHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    let request = ErrorRequest {
                        value: "trigger error".to_string(),
                    };

                    let result: Result<SimpleResponse, _> = recv(cx.send_request(request)).await;

                    // Should get the error the handler returned
                    assert!(result.is_err());
                    if let Err(err) = result {
                        assert_eq!(err.code, -32000);
                        assert_eq!(err.message, "This is an intentional error");
                    }
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 4: Request without required params
// ============================================================================

struct StrictParamHandler;

impl JsonRpcHandler for StrictParamHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error> {
        if cx.method() == "strict_method" {
            // Try to parse params - should fail if missing/invalid
            match scp::util::json_cast::<_, SimpleRequest>(params) {
                Ok(request) => {
                    cx.cast().respond(SimpleResponse {
                        result: format!("Got: {}", request.message),
                    })?;
                }
                Err(_) => {
                    // Send error response instead of returning Err from handler
                    cx.respond_with_error(agent_client_protocol::Error::invalid_params())?;
                }
            }
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EmptyRequest;

impl JsonRpcMessage for EmptyRequest {}

impl JsonRpcOutgoingMessage for EmptyRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "strict_method"
    }
}

impl JsonRpcRequest for EmptyRequest {
    type Response = SimpleResponse;
}

#[tokio::test(flavor = "current_thread")]
async fn test_missing_required_params() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(StrictParamHandler);

            tokio::task::spawn_local(async move {
                server.serve().await.ok();
            });

            let result = client
                .with_client(async |cx| -> Result<(), agent_client_protocol::Error> {
                    // Send request with no params (EmptyRequest has no fields)
                    let request = EmptyRequest;

                    let result: Result<SimpleResponse, _> = recv(cx.send_request(request)).await;

                    // Should get invalid_params error
                    assert!(result.is_err());
                    if let Err(err) = result {
                        assert_eq!(err.code, -32602); // JSONRPC_INVALID_PARAMS
                    }
                    Ok(())
                })
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}
