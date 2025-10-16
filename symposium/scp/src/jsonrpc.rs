use std::marker::PhantomData;
use std::pin::Pin;

use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::{AsyncRead, AsyncWrite};
use serde::de::DeserializeOwned;

mod actors;

/// Create a JsonRpcConnection. This can be the basis for either a server or a client.
#[must_use]
pub struct JsonRpcConnection {
    outgoing_bytes: Pin<Box<dyn AsyncWrite>>,
    incoming_bytes: Pin<Box<dyn AsyncRead>>,
    outgoing_rx: mpsc::UnboundedReceiver<OutgoingMessage>,
    outgoing_tx: mpsc::UnboundedSender<OutgoingMessage>,
    layers: Vec<Box<dyn JsonRpcHandler>>,
}

impl JsonRpcConnection {
    pub fn new(
        outgoing_bytes: impl AsyncWrite + 'static,
        incoming_bytes: impl AsyncRead + 'static,
    ) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
        Self {
            outgoing_bytes: Box::pin(outgoing_bytes),
            incoming_bytes: Box::pin(incoming_bytes),
            outgoing_rx,
            outgoing_tx,
            layers: Vec::new(),
        }
    }

    /// Adds a message handler that will have the opportunity to process incoming messages.
    /// When a new message arrives, handlers are tried in the order they were added, and
    /// the first to "claim" the message "wins".
    pub fn add_handler(mut self, layer: impl JsonRpcHandler + 'static) -> Self {
        self.layers.push(Box::new(layer));
        self
    }

    /// Returns a [`JsonRpcCx`] that allows you to send requests over the connection
    /// and receive responses.
    fn json_rpc_cx(&self) -> JsonRpcCx {
        JsonRpcCx::new(self.outgoing_tx.clone())
    }

    /// Runs a server that listens for incoming requests and handles them according to the added handlers.
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let (reply_tx, reply_rx) = mpsc::unbounded();
        let (incoming_cancel_tx, incoming_cancel_rx) = oneshot::channel();
        let json_rpc_cx = JsonRpcCx::new(self.outgoing_tx);
        let (r1, r2, r3) = futures::join!(
            actors::outgoing_actor(
                self.outgoing_rx,
                reply_tx.clone(),
                self.outgoing_bytes,
                incoming_cancel_rx
            ),
            actors::incoming_actor(
                &json_rpc_cx,
                self.incoming_bytes,
                reply_tx,
                incoming_cancel_tx,
                self.layers
            ),
            actors::reply_actor(reply_rx),
        );
        r1?;
        r2?;
        r3?;
        Ok(())
    }

    /// Serves messages over the connection until `main_fn` returns, then the connection will be dropped.
    /// Incoming messages will be handled according to the added handlers.
    ///
    /// [`main_fn`] is invoked with a [`JsonRpcCx`] that allows you to send requests over the connection
    /// and receive responses.
    ///
    /// Errors if the server terminates before `main_fn` returns.
    pub async fn with_client(
        self,
        main_fn: impl AsyncFnOnce(JsonRpcCx) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cx = self.json_rpc_cx();

        // Run the server + the main function until one terminates.
        // We EXPECT the main function to be the one to terminate
        // except in case of error.
        let result = futures::future::select(Box::pin(self.serve()), Box::pin(main_fn(cx))).await;

        match result {
            Either::Left((Ok(()), _)) => Err(format!("server unexpectedly shut down").into()),

            Either::Left((result, _)) | Either::Right((result, _)) => result,
        }
    }
}

/// Message sent to the reply management actor
enum ReplyMessage {
    /// Wait for a response to the given id and then send it to the given receiver
    Subscribe(
        jsonrpcmsg::Id,
        oneshot::Sender<Result<serde_json::Value, jsonrpcmsg::Error>>,
    ),

    /// Dispatch a response to the given id and value
    Dispatch(jsonrpcmsg::Id, Result<serde_json::Value, jsonrpcmsg::Error>),
}

/// Messages send to be serialized over the transport.
enum OutgoingMessage {
    /// Send a request to the server.
    Request {
        /// method to use in the request
        method: String,

        /// parameters for the request
        params: Option<jsonrpcmsg::Params>,

        /// where to send the response when it arrives
        response_tx: oneshot::Sender<Result<serde_json::Value, jsonrpcmsg::Error>>,
    },

    /// Send a notification to the server.
    Notification {
        /// method to use in the request
        method: String,

        /// parameters for the request
        params: Option<jsonrpcmsg::Params>,
    },

    /// Send a reponse to a message from the server
    Response {
        id: jsonrpcmsg::Id,

        response: Result<serde_json::Value, jsonrpcmsg::Error>,
    },

    /// Send a generalized error message
    Error { error: jsonrpcmsg::Error },
}

/// Handlers are invoked when new messages arrive at the [`JsonRpcServer`].
/// They have a chance to inspect the method and parameters and decide whether to "claim" the request
/// (i.e., handle it). If they do not claim it, the request will be passed to the next handler.
pub trait JsonRpcHandler {
    /// Attempt to claim the incoming request (`method`/`params`).
    /// Returns `Ok(())` if the request was claimed, `Err(cx)` if not.
    /// If the request was claimed, the handler should send a response using the provided context.
    /// If the request is not claimed, the handler should return the request context so it can be passed to the next handler.
    fn claim_request(
        &self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<jsonrpcmsg::Response>,
    ) -> Result<(), JsonRpcRequestCx<jsonrpcmsg::Response>>;

    /// Attempt to claim a notification (`method`/`params`).
    /// Returns `Ok(())` if the notification was handled, `Err(())` if not.
    fn claim_notification(
        &self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<(), ()>;
}

/// The context given when an incoming message arrives.
/// Used to respond a response of type `T`.
#[derive(Clone)]
pub struct JsonRpcCx {
    tx: mpsc::UnboundedSender<OutgoingMessage>,
}

impl JsonRpcCx {
    fn new(tx: mpsc::UnboundedSender<OutgoingMessage>) -> Self {
        Self { tx }
    }

    /// Send an outgoing request and await the reply.
    pub fn send_request<R>(&self, request: impl JsonRpcRequest<Response = R>) -> JsonRpcResponse<R>
    where
        R: DeserializeOwned,
    {
        let (response_tx, response_rx) = oneshot::channel();
        let method = request.method();
        let result = serde_json::to_value(request.into_params())
            .and_then(|json| serde_json::from_value::<Option<jsonrpcmsg::Params>>(json));

        match result {
            Ok(params) => {
                let message = OutgoingMessage::Request {
                    method,
                    params,
                    response_tx,
                };

                match self.tx.unbounded_send(message) {
                    Ok(()) => (),
                    Err(error) => {
                        let OutgoingMessage::Request {
                            method,
                            response_tx,
                            ..
                        } = error.into_inner()
                        else {
                            unreachable!();
                        };

                        response_tx
                            .send(Err(communication_failure(format!(
                                "failed to send outgoing request `{method}"
                            ))))
                            .unwrap();
                    }
                }
            }

            Err(_) => {
                response_tx
                    .send(Err(communication_failure(format!(
                        "failed to send outgoing request `{method}"
                    ))))
                    .unwrap();
            }
        }

        JsonRpcResponse::new(response_rx)
    }

    /// Send an outgoing notification (no reply expected).)
    pub fn send_notification<R>(
        &self,
        notification: impl JsonRpcNotification,
    ) -> Result<(), jsonrpcmsg::Error> {
        let method = notification.method();
        let params: Option<jsonrpcmsg::Params> = serde_json::to_value(notification.into_params())
            .and_then(|json| serde_json::from_value(json))
            .map_err(|err| jsonrpcmsg::Error::new(JSONRPC_INVALID_PARAMS, err.to_string()))?;

        self.send_raw_message(OutgoingMessage::Notification { method, params })
    }

    /// Send an error notification (no reply expected).
    pub fn send_error_notification(
        &self,
        error: jsonrpcmsg::Error,
    ) -> Result<(), jsonrpcmsg::Error> {
        self.send_raw_message(OutgoingMessage::Error { error })
    }

    fn send_raw_message(&self, message: OutgoingMessage) -> Result<(), jsonrpcmsg::Error> {
        self.tx
            .unbounded_send(message)
            .map_err(communication_failure)
    }
}

/// The context to respond to an incoming request.
/// Derefs to a [`JsonRpcCx`] which can be used to send other requests and notification.
#[must_use]
pub struct JsonRpcRequestCx<T: serde::Serialize> {
    cx: JsonRpcCx,
    id: jsonrpcmsg::Id,
    data: PhantomData<mpsc::UnboundedSender<T>>,
}

impl<T: serde::Serialize> std::ops::Deref for JsonRpcRequestCx<T> {
    type Target = JsonRpcCx;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}

impl<T: serde::Serialize> JsonRpcRequestCx<T> {
    fn new(cx: JsonRpcCx, id: jsonrpcmsg::Id) -> Self {
        Self {
            cx,
            id,
            data: PhantomData,
        }
    }

    /// Return a new JsonRpcResponse that expects a response of type U.
    pub fn expect<U: serde::Serialize>(self) -> JsonRpcRequestCx<U> {
        JsonRpcRequestCx {
            id: self.id,
            cx: self.cx,
            data: PhantomData,
        }
    }

    /// Get the underlying JSON RPC context.
    pub fn json_rpc_cx(&self) -> JsonRpcCx {
        self.cx.clone()
    }

    /// Respond to the JSON-RPC request with a value.
    pub fn respond(self, response: T) -> Result<(), jsonrpcmsg::Error> {
        let Ok(value) = serde_json::to_value(response) else {
            return self.respond_with_internal_error();
        };

        self.cx.send_raw_message(OutgoingMessage::Response {
            id: self.id,
            response: Ok(value),
        })
    }

    /// Respond to the JSON-RPC request with an internal error.
    pub fn respond_with_internal_error(self) -> Result<(), jsonrpcmsg::Error> {
        self.respond_with_error(jsonrpcmsg::Error::internal_error())
    }

    /// Respond to the JSON-RPC request with an error.
    pub fn respond_with_error(self, error: jsonrpcmsg::Error) -> Result<(), jsonrpcmsg::Error> {
        self.cx.send_raw_message(OutgoingMessage::Response {
            id: self.id,
            response: Err(error),
        })
    }
}

///A struct that serializes to the paramcontaining the parameters
pub trait JsonRpcNotification {
    /// The method name for the request.
    fn method(&self) -> String;

    /// Value which will be serialized to product the request parameters.
    fn into_params(self) -> impl serde::Serialize;
}

///A struct that serializes to the paramcontaining the parameters
pub trait JsonRpcRequest {
    /// The type of data expected in response.
    type Response: serde::de::DeserializeOwned;

    /// The method name for the request.
    fn method(&self) -> String;

    /// Value which will be serialized to product the request parameters.
    fn into_params(self) -> impl serde::Serialize;
}

/// Represents a pending response of type `R` from an outgoing request.
pub struct JsonRpcResponse<R: DeserializeOwned> {
    response_rx: oneshot::Receiver<Result<serde_json::Value, jsonrpcmsg::Error>>,
    data: PhantomData<oneshot::Receiver<Result<R, jsonrpcmsg::Error>>>,
}

impl<R: DeserializeOwned> JsonRpcResponse<R> {
    fn new(response_rx: oneshot::Receiver<Result<serde_json::Value, jsonrpcmsg::Error>>) -> Self {
        Self {
            response_rx,
            data: PhantomData,
        }
    }

    /// Wait for the response to arrive.
    pub async fn recv(self) -> Result<R, jsonrpcmsg::Error> {
        // Wait for the JSON to be sent by the other side.
        let json_value = self.response_rx.await.map_err(|_| {
            // If the sender is dropped without a message...
            jsonrpcmsg::Error::server_error(
                COMMUNICATION_FAILURE,
                format!(
                    "reply of type `{}` never arrived",
                    std::any::type_name::<R>()
                ),
            )
        })??;

        // Deserialize into the expected type R
        serde_json::from_value(json_value)
            .map_err(|err| jsonrpcmsg::Error::new(JSONRPC_INVALID_PARAMS, err.to_string()))
    }
}

const JSONRPC_INVALID_PARAMS: i32 = -32602;
const COMMUNICATION_FAILURE: i32 = -32000;

fn communication_failure(err: impl ToString) -> jsonrpcmsg::Error {
    jsonrpcmsg::Error::new(COMMUNICATION_FAILURE, err.to_string())
}
