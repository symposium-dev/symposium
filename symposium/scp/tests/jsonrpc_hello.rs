//! Integration test for basic JSON-RPC communication.
//!
//! This test sets up two JSON-RPC connections and verifies they can
//! exchange simple "hello world" messages.

use scp::jsonrpc::{
    Handled, JsonRpcConnection, JsonRpcCx, JsonRpcHandler, JsonRpcNotification, JsonRpcRequestCx,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Helper to set up a client-server pair for testing.
/// Returns (server_connection, client_connection).
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

/// A simple "ping" request.
#[derive(Debug, Serialize, Deserialize)]
struct PingRequest {
    message: String,
}

impl scp::jsonrpc::JsonRpcRequest for PingRequest {
    type Response = PongResponse;

    fn method(&self) -> &str {
        "ping"
    }
}

/// A simple "pong" response.
#[derive(Debug, Serialize, Deserialize)]
struct PongResponse {
    echo: String,
}

/// Handler that responds to ping requests.
struct PingHandler;

impl JsonRpcHandler for PingHandler {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> std::result::Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method == "ping" {
            // Parse the request
            let request: PingRequest =
                scp::util::json_cast(params).map_err(|_| jsonrpcmsg::Error::invalid_params())?;

            // Send back a pong
            let pong = PongResponse {
                echo: format!("pong: {}", request.message),
            };

            response.cast::<PongResponse>().respond(pong)?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
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
                    eprintln!("Server error: {}", e);
                }
            });

            // Use the client to send a ping and wait for a pong
            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), Box<dyn std::error::Error>> {
                        let request = PingRequest {
                            message: "hello world".to_string(),
                        };

                        let response = cx.send_request(request).recv().await.map_err(
                            |e| -> Box<dyn std::error::Error> {
                                format!("Request failed: {:?}", e).into()
                            },
                        )?;

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

impl JsonRpcNotification for LogNotification {
    fn method(&self) -> &str {
        "log"
    }
}

/// Handler that collects log notifications
struct LogHandler {
    logs: Arc<Mutex<Vec<String>>>,
}

impl JsonRpcHandler for LogHandler {
    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        _cx: &JsonRpcCx,
    ) -> std::result::Result<Handled<()>, jsonrpcmsg::Error> {
        if method == "log" {
            let log: LogNotification =
                scp::util::json_cast(params).map_err(|_| jsonrpcmsg::Error::invalid_params())?;

            self.logs.lock().unwrap().push(log.message);
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(()))
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
                    eprintln!("Server error: {}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), Box<dyn std::error::Error>> {
                        // Send a notification (no response expected)
                        cx.send_notification(LogNotification {
                            message: "test log 1".to_string(),
                        })
                        .map_err(|e| -> Box<dyn std::error::Error> {
                            format!("Failed to send notification: {:?}", e).into()
                        })?;

                        cx.send_notification(LogNotification {
                            message: "test log 2".to_string(),
                        })
                        .map_err(|e| -> Box<dyn std::error::Error> {
                            format!("Failed to send notification: {:?}", e).into()
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
                    eprintln!("Server error: {}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), Box<dyn std::error::Error>> {
                        // Send multiple requests sequentially
                        for i in 1..=5 {
                            let request = PingRequest {
                                message: format!("message {}", i),
                            };

                            let response = cx.send_request(request).recv().await.map_err(
                                |e| -> Box<dyn std::error::Error> {
                                    format!("Request {} failed: {:?}", i, e).into()
                                },
                            )?;

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
                    eprintln!("Server error: {}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), Box<dyn std::error::Error>> {
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
                            let response = response_future.recv().await.map_err(
                                |e| -> Box<dyn std::error::Error> {
                                    format!("Request {} failed: {:?}", i, e).into()
                                },
                            )?;

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
