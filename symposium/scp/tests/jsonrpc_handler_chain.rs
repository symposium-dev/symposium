//! Integration tests for JSON-RPC handler chain behavior.
//!
//! These tests verify that multiple handlers can be chained together
//! and that requests/notifications are routed correctly based on which
//! handler claims them.

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

// ============================================================================
// Test 1: Multiple handlers with different methods
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct FooRequest {
    value: String,
}

impl JsonRpcMessage for FooRequest {}

impl JsonRpcOutgoingMessage for FooRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "foo"
    }
}

impl JsonRpcRequest for FooRequest {
    type Response = FooResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct FooResponse {
    result: String,
}

impl JsonRpcMessage for FooResponse {}

impl JsonRpcIncomingMessage for FooResponse {
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
struct BarRequest {
    value: String,
}

impl JsonRpcMessage for BarRequest {}

impl JsonRpcOutgoingMessage for BarRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "bar"
    }
}

impl JsonRpcRequest for BarRequest {
    type Response = BarResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct BarResponse {
    result: String,
}

impl JsonRpcMessage for BarResponse {}

impl JsonRpcIncomingMessage for BarResponse {
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

struct FooHandler;

impl JsonRpcHandler for FooHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<
        Handled<JsonRpcRequestCx<serde_json::Value>>,
        agent_client_protocol::Error,
    > {
        if cx.method() == "foo" {
            let request: FooRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            cx.cast().respond(FooResponse {
                result: format!("foo: {}", request.value),
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

struct BarHandler;

impl JsonRpcHandler for BarHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<
        Handled<JsonRpcRequestCx<serde_json::Value>>,
        agent_client_protocol::Error,
    > {
        if cx.method() == "bar" {
            let request: BarRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            cx.cast().respond(BarResponse {
                result: format!("bar: {}", request.value),
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_multiple_handlers_different_methods() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();
            let client_reader = client_reader.compat();
            let client_writer = client_writer.compat_write();

            // Chain both handlers
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive(FooHandler)
                .on_receive(BarHandler);
            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {e:?}");
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Test foo request
                        let foo_response = recv(cx.send_request(FooRequest {
                            value: "test1".to_string(),
                        }))
                        .await
                        .map_err(
                            |e| -> agent_client_protocol::Error {
                                scp::util::internal_error(format!("Foo request failed: {e:?}"))
                            },
                        )?;
                        assert_eq!(foo_response.result, "foo: test1");

                        // Test bar request
                        let bar_response = recv(cx.send_request(BarRequest {
                            value: "test2".to_string(),
                        }))
                        .await
                        .map_err(
                            |e| -> agent_client_protocol::Error {
                                scp::util::internal_error(format!("Bar request failed: {:?}", e))
                            },
                        )?;
                        assert_eq!(bar_response.result, "bar: test2");

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 2: Handler priority/ordering (first handler gets first chance)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct TrackRequest {
    value: String,
}

impl JsonRpcMessage for TrackRequest {}

impl JsonRpcOutgoingMessage for TrackRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "track"
    }
}

impl JsonRpcRequest for TrackRequest {
    type Response = FooResponse;
}

struct TrackingHandler {
    name: String,
    handled: Arc<Mutex<Vec<String>>>,
}

impl JsonRpcHandler for TrackingHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<
        Handled<JsonRpcRequestCx<serde_json::Value>>,
        agent_client_protocol::Error,
    > {
        if cx.method() == "track" {
            self.handled.lock().unwrap().push(self.name.clone());

            let request: TrackRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            cx.cast().respond(FooResponse {
                result: format!("{}: {}", self.name, request.value),
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_handler_priority_ordering() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let handled = Arc::new(Mutex::new(Vec::new()));

            let (client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();
            let client_reader = client_reader.compat();
            let client_writer = client_writer.compat_write();

            // First handler in chain should get first chance
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive(TrackingHandler {
                    name: "handler1".to_string(),
                    handled: handled.clone(),
                })
                .on_receive(TrackingHandler {
                    name: "handler2".to_string(),
                    handled: handled.clone(),
                });
            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        let response = recv(cx.send_request(TrackRequest {
                            value: "test".to_string(),
                        }))
                        .await
                        .map_err(|e| {
                            scp::util::internal_error(format!("Track request failed: {:?}", e))
                        })?;

                        // First handler should have handled it
                        assert_eq!(response.result, "handler1: test");

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);

            // Verify only handler1 was invoked
            let handled_by = handled.lock().unwrap();
            assert_eq!(handled_by.len(), 1);
            assert_eq!(handled_by[0], "handler1");
        })
        .await;
}

// ============================================================================
// Test 3: Fallthrough behavior (handler passes to next)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct Method1Request {
    value: String,
}

impl JsonRpcMessage for Method1Request {}

impl JsonRpcOutgoingMessage for Method1Request {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "method1"
    }
}

impl JsonRpcRequest for Method1Request {
    type Response = FooResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct Method2Request {
    value: String,
}

impl JsonRpcMessage for Method2Request {}

impl JsonRpcOutgoingMessage for Method2Request {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "method2"
    }
}

impl JsonRpcRequest for Method2Request {
    type Response = FooResponse;
}

struct SelectiveHandler {
    method_to_handle: String,
    handled: Arc<Mutex<Vec<String>>>,
}

impl JsonRpcHandler for SelectiveHandler {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<
        Handled<JsonRpcRequestCx<serde_json::Value>>,
        agent_client_protocol::Error,
    > {
        if cx.method() == self.method_to_handle {
            let method = cx.method().to_string();
            self.handled.lock().unwrap().push(method.clone());

            // Parse as generic struct with value field
            #[derive(Deserialize)]
            struct GenericRequest {
                value: String,
            }
            let request: GenericRequest = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            cx.cast().respond(FooResponse {
                result: format!("{}: {}", method, request.value),
            })?;
            Ok(Handled::Yes)
        } else {
            // Pass through to next handler
            Ok(Handled::No(cx))
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_fallthrough_behavior() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let handled = Arc::new(Mutex::new(Vec::new()));

            let (client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();
            let client_reader = client_reader.compat();
            let client_writer = client_writer.compat_write();

            // Handler1 only handles "method1", Handler2 only handles "method2"
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive(SelectiveHandler {
                    method_to_handle: "method1".to_string(),
                    handled: handled.clone(),
                })
                .on_receive(SelectiveHandler {
                    method_to_handle: "method2".to_string(),
                    handled: handled.clone(),
                });
            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Send method2 - should fallthrough handler1 to handler2
                        let response = recv(cx.send_request(Method2Request {
                            value: "fallthrough".to_string(),
                        }))
                        .await
                        .map_err(|e| {
                            scp::util::internal_error(format!("Method2 request failed: {:?}", e))
                        })?;

                        assert_eq!(response.result, "method2: fallthrough");

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);

            // Verify only method2 was handled (handler1 passed through)
            let handled_methods = handled.lock().unwrap();
            assert_eq!(handled_methods.len(), 1);
            assert_eq!(handled_methods[0], "method2");
        })
        .await;
}

// ============================================================================
// Test 4: No handler claims request
// ============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_no_handler_claims() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let (client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();
            let client_reader = client_reader.compat();
            let client_writer = client_writer.compat_write();

            // Handler that only handles "foo"
            let server =
                JsonRpcConnection::new(server_writer, server_reader).on_receive(FooHandler);
            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        // Send "bar" request which no handler claims
                        let response_result = recv(cx.send_request(BarRequest {
                            value: "unclaimed".to_string(),
                        }))
                        .await;

                        // Should get an error (method not found)
                        assert!(response_result.is_err());

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);
        })
        .await;
}

// ============================================================================
// Test 5: Handler can claim notifications
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct EventNotification {
    event: String,
}

impl JsonRpcMessage for EventNotification {}

impl JsonRpcOutgoingMessage for EventNotification {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        scp::util::json_cast(self)
    }

    fn method(&self) -> &str {
        "event"
    }
}

impl JsonRpcNotification for EventNotification {}

struct EventHandler {
    events: Arc<Mutex<Vec<String>>>,
}

impl JsonRpcHandler for EventHandler {
    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<Handled<JsonRpcNotificationCx>, agent_client_protocol::Error> {
        if cx.method() == "event" {
            let notification: EventNotification = scp::util::json_cast(params)
                .map_err(|_| agent_client_protocol::Error::invalid_params())?;

            self.events.lock().unwrap().push(notification.event);
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(cx))
        }
    }
}

struct IgnoreHandler;

impl JsonRpcHandler for IgnoreHandler {
    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> std::result::Result<Handled<JsonRpcNotificationCx>, agent_client_protocol::Error> {
        // Never claims anything, always passes through
        Ok(Handled::No(cx))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_handler_claims_notification() {
    use tokio::task::LocalSet;

    let local = LocalSet::new();

    local
        .run_until(async {
            let events = Arc::new(Mutex::new(Vec::new()));

            let (client_writer, server_reader) = tokio::io::duplex(1024);
            let (server_writer, client_reader) = tokio::io::duplex(1024);

            let server_reader = server_reader.compat();
            let server_writer = server_writer.compat_write();
            let client_reader = client_reader.compat();
            let client_writer = client_writer.compat_write();

            // IgnoreHandler passes through, EventHandler claims
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive(IgnoreHandler)
                .on_receive(EventHandler {
                    events: events.clone(),
                });
            let client = JsonRpcConnection::new(client_writer, client_reader);

            tokio::task::spawn_local(async move {
                if let Err(e) = server.serve().await {
                    eprintln!("Server error: {:?}", e);
                }
            });

            let result = client
                .with_client(
                    async |cx| -> std::result::Result<(), agent_client_protocol::Error> {
                        cx.send_notification(EventNotification {
                            event: "test_event".to_string(),
                        })
                        .map_err(|e| {
                            scp::util::internal_error(format!(
                                    "Failed to send notification: {:?}",
                                    e
                                ))
                        })?;

                        // Give server time to process
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        Ok(())
                    },
                )
                .await;

            assert!(result.is_ok(), "Test failed: {:?}", result);

            let received_events = events.lock().unwrap();
            assert_eq!(received_events.len(), 1);
            assert_eq!(received_events[0], "test_event");
        })
        .await;
}
