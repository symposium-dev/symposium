//! Integration test for basic JSON-RPC communication.
//!
//! This test sets up two JSON-RPC connections and verifies they can
//! exchange simple "hello world" messages.

use futures::{AsyncRead, AsyncWrite};
use scp::{
    Handled, JsonRpcConnection, JsonRpcHandler, JsonRpcIncomingMessage, JsonRpcMessage,
    JsonRpcNotification, JsonRpcNotificationCx, JsonRpcOutgoingMessage, JsonRpcRequest,
    JsonRpcRequestCx, JsonRpcResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
/// Returns (server_connection, client_connection).
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

/// A simple "ping" request.
#[derive(Debug, Serialize, Deserialize)]
struct PingRequest {
    message: String,
}

impl JsonRpcMessage for PingRequest {}

impl JsonRpcOutgoingMessage for PingRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "ping"
    }
}

impl JsonRpcRequest for PingRequest {
    type Response = PongResponse;
}

/// A simple "pong" response.
#[derive(Debug, Serialize, Deserialize)]
struct PongResponse {
    echo: String,
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

/// Handler that responds to ping requests.
struct PingHandler;

impl JsonRpcHandler for PingHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<
        Handled<JsonRpcRequestCx<serde_json::Value>>,
        agent_client_protocol::Error,
    > {
        if cx.method() == "ping" {
            // Parse the request
            let request: PingRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            // Send back a pong
            let pong = PongResponse {
                echo: format!("pong: {}", request.message),
            };

            cx.cast().respond(pong)?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_hello_world() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(PingHandler);

            // Spawn the server in the background
            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            // Use the client to send a ping and wait for a pong
            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        let request = PingRequest {
                            message: "hello world".to_string(),
                        };

                        let response = recv(cx.send_request(request)).await.map_err(|e| {
                            agent_client_protocol::Error::into_internal_error(
                                std::io::Error::other(format!("Request failed: {:?}", e)),
                            )
                        })?;

                        assert_eq!(response.echo, "pong: hello world");

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

/// A simple notification message
#[derive(Debug, Serialize, Deserialize)]
struct LogNotification {
    message: String,
}

impl JsonRpcMessage for LogNotification {}

impl JsonRpcOutgoingMessage for LogNotification {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "log"
    }
}

impl JsonRpcNotification for LogNotification {}

/// Handler that collects log notifications
struct LogHandler {
    logs: Arc<Mutex<Vec<String>>>,
}

impl JsonRpcHandler for LogHandler {
    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<Handled<JsonRpcNotificationCx>, agent_client_protocol::Error> {
        if cx.method() == "log" {
            let log: LogNotification = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            self.logs.lock().unwrap().push(log.message);
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_notification() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let logs = Arc::new(Mutex::new(Vec::new()));
            let logs_clone = logs.clone();

            let (server, client) = setup_test_connections(LogHandler { logs: logs_clone });

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Send a notification (no response expected)
                        cx.send_notification(LogNotification {
                            message: "test log 1".to_string(),
                        })
                        .map_err(|e| {
                            agent_client_protocol::Error::into_internal_error(
                                std::io::Error::other(format!(
                                    "Failed to send notification: {:?}",
                                    e
                                )),
                            )
                        })?;

                        cx.send_notification(LogNotification {
                            message: "test log 2".to_string(),
                        })
                        .map_err(|e| {
                            agent_client_protocol::Error::into_internal_error(
                                std::io::Error::other(format!(
                                    "Failed to send notification: {:?}",
                                    e
                                )),
                            )
                        })?;

                        // Give the server time to process notifications
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);

            let received_logs = logs.lock().unwrap();
            assert_eq!(received_logs.len(), 2);
            assert_eq!(received_logs[0], "test log 1");
            assert_eq!(received_logs[1], "test log 2");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn test_multiple_sequential_requests() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(PingHandler);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Send multiple requests sequentially
                        for i in 1..=5 {
                            let request = PingRequest {
                                message: format!("message {}", i),
                            };

                            let response = recv(cx.send_request(request)).await.map_err(|e| {
                                agent_client_protocol::Error::into_internal_error(
                                    std::io::Error::other(format!("Request {} failed: {:?}", i, e)),
                                )
                            })?;

                            assert_eq!(response.echo, format!("pong: message {}", i));
                        }

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_requests() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (server, client) = setup_test_connections(PingHandler);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Send multiple requests concurrently
                        let mut responses = Vec::new();

                        for i in 1..=5 {
                            let request = PingRequest {
                                message: format!("concurrent message {}", i),
                            };

                            // Start all requests without awaiting
                            responses.push((i, cx.send_request(request)));
                        }

                        // Now await all responses
                        for (i, response_future) in responses {
                            let response = recv(response_future).await.map_err(|e| {
                                agent_client_protocol::Error::into_internal_error(
                                    std::io::Error::other(format!("Request {} failed: {:?}", i, e)),
                                )
                            })?;

                            assert_eq!(response.echo, format!("pong: concurrent message {}", i));
                        }

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}
