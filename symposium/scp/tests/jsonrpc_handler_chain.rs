//! Integration tests for JSON-RPC handler chain behavior.
//!
//! These tests verify that multiple handlers can be chained together
//! and that requests/notifications are routed correctly based on which
//! handler claims them.

use scp::{
    JsonRpcConnection, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcRequestCx,
    JsonRpcResponse, JsonRpcResponsePayload,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

// ============================================================================
// Test 1: Multiple handlers with different methods
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct FooRequest {
    value: String,
}

impl JsonRpcMessage for FooRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "foo"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "foo" {
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

impl JsonRpcRequest for FooRequest {
    type Response = FooResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct FooResponse {
    result: String,
}

impl JsonRpcResponsePayload for FooResponse {
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

impl JsonRpcMessage for BarRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "bar"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "bar" {
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

impl JsonRpcRequest for BarRequest {
    type Response = BarResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct BarResponse {
    result: String,
}

impl JsonRpcResponsePayload for BarResponse {
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
                .on_receive_request(
                    async |request: FooRequest, request_cx: JsonRpcRequestCx<FooResponse>| {
                        request_cx.respond(FooResponse {
                            result: format!("foo: {}", request.value),
                        })
                    },
                )
                .on_receive_request(
                    async |request: BarRequest, request_cx: JsonRpcRequestCx<BarResponse>| {
                        request_cx.respond(BarResponse {
                            result: format!("bar: {}", request.value),
                        })
                    },
                );
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

impl JsonRpcMessage for TrackRequest {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "track"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "track" {
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

impl JsonRpcRequest for TrackRequest {
    type Response = FooResponse;
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
            let handled_clone1 = handled.clone();
            let handled_clone2 = handled.clone();
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive_request(
                    async move |request: TrackRequest,
                                request_cx: JsonRpcRequestCx<FooResponse>| {
                        handled_clone1.lock().unwrap().push("handler1".to_string());
                        request_cx.respond(FooResponse {
                            result: format!("handler1: {}", request.value),
                        })
                    },
                )
                .on_receive_request(
                    async move |request: TrackRequest,
                                request_cx: JsonRpcRequestCx<FooResponse>| {
                        handled_clone2.lock().unwrap().push("handler2".to_string());
                        request_cx.respond(FooResponse {
                            result: format!("handler2: {}", request.value),
                        })
                    },
                );
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

impl JsonRpcMessage for Method1Request {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "method1"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "method1" {
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

impl JsonRpcRequest for Method1Request {
    type Response = FooResponse;
}

#[derive(Debug, Serialize, Deserialize)]
struct Method2Request {
    value: String,
}

impl JsonRpcMessage for Method2Request {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "method2"
    }

    fn parse_request(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "method2" {
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

impl JsonRpcRequest for Method2Request {
    type Response = FooResponse;
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
            let handled_clone1 = handled.clone();
            let handled_clone2 = handled.clone();
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive_request(
                    async move |request: Method1Request,
                                request_cx: JsonRpcRequestCx<FooResponse>| {
                        handled_clone1.lock().unwrap().push("method1".to_string());
                        request_cx.respond(FooResponse {
                            result: format!("method1: {}", request.value),
                        })
                    },
                )
                .on_receive_request(
                    async move |request: Method2Request,
                                request_cx: JsonRpcRequestCx<FooResponse>| {
                        handled_clone2.lock().unwrap().push("method2".to_string());
                        request_cx.respond(FooResponse {
                            result: format!("method2: {}", request.value),
                        })
                    },
                );
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
            let server = JsonRpcConnection::new(server_writer, server_reader).on_receive_request(
                async |request: FooRequest, request_cx: JsonRpcRequestCx<FooResponse>| {
                    request_cx.respond(FooResponse {
                        result: format!("foo: {}", request.value),
                    })
                },
            );
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

impl JsonRpcMessage for EventNotification {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        scp::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "event"
    }

    fn parse_request(
        _method: &str,
        _params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a notification, not a request
        None
    }

    fn parse_notification(
        method: &str,
        params: &impl serde::Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "event" {
            return None;
        }
        Some(scp::util::json_cast(params))
    }
}

impl JsonRpcNotification for EventNotification {}

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

            // EventHandler claims notifications
            let events_clone = events.clone();
            let server = JsonRpcConnection::new(server_writer, server_reader)
                .on_receive_notification(
                    async move |notification: EventNotification, _notification_cx| {
                        events_clone.lock().unwrap().push(notification.event);
                        Ok(())
                    },
                );
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
