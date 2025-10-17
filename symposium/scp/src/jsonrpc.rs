//! Core JSON-RPC server support.

use std::marker::PhantomData;
use std::pin::Pin;

use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::{AsyncRead, AsyncWrite};
use serde::de::DeserializeOwned;

use crate::util::json_cast;

mod actors;
mod handlers;
pub use handlers::*;

/// Create a JsonRpcConnection. This can be the basis for either a server or a client.
#[must_use]
pub struct JsonRpcConnection<H: JsonRpcHandler> {
    /// Where to send bytes to communicate to the other side
    outgoing_bytes: Pin<Box<dyn AsyncWrite>>,

    /// Where to read bytes from the other side
    incoming_bytes: Pin<Box<dyn AsyncRead>>,

    /// Where the "outgoing messages" actor will receive messages.
    outgoing_rx: mpsc::UnboundedReceiver<OutgoingMessage>,

    /// Sender to send messages to the "outgoing message" actor.
    outgoing_tx: mpsc::UnboundedSender<OutgoingMessage>,

    /// Handler for incoming messages.
    handler: H,
}

impl JsonRpcConnection<NullHandler> {
    /// Create a new JsonRpcConnection that will read and write from the given streams.
    /// This type follows a builder pattern; use other methods to configure and then invoke
    /// [`Self:serve`] (to use as a server) or [`Self::with_client`] to use as a client.
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
            handler: NullHandler::default(),
        }
    }
}

impl<H: JsonRpcHandler> JsonRpcConnection<H> {
    /// Adds a message handler that will have the opportunity to process incoming messages.
    /// When a new message arrives, handlers are tried in the order they were added, and
    /// the first to "claim" the message "wins".
    pub fn on_receive<H1>(self, handler: H1) -> JsonRpcConnection<ChainHandler<H, H1>>
    where
        H1: JsonRpcHandler,
    {
        JsonRpcConnection {
            handler: ChainHandler::new(self.handler, handler),
            outgoing_bytes: self.outgoing_bytes,
            incoming_bytes: self.incoming_bytes,
            outgoing_rx: self.outgoing_rx,
            outgoing_tx: self.outgoing_tx,
        }
    }

    /// Returns a [`JsonRpcCx`] that allows you to send requests over the connection
    /// and receive responses.
    ///
    /// This is private because it would give people a footgun if they had it,
    /// since they might try to use it when the server is not running and deadlock,
    /// and I don't really think they need it.
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
                self.handler,
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
#[allow(async_fn_in_trait)]
pub trait JsonRpcHandler {
    /// Attempt to claim the incoming request (`method`/`params`).
    ///
    /// # Important
    ///
    /// The server will not process new messages until this handler returns.
    /// You should avoid blocking in this callback unless you wish to block the server (e.g., for rate limiting).
    /// The recommended approach to manage expensive operations is to use a channel or spawn tasks.
    ///
    /// # Returns
    ///
    /// * `Ok(Handled::Yes)` if the request was claimed.
    /// * `Ok(Handled::No(response))` if not.
    /// * `Err` if an internal error occurs (this will bring down the server).
    #[allow(unused_variables)]
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<jsonrpcmsg::Response>,
    ) -> Result<Handled<JsonRpcRequestCx<jsonrpcmsg::Response>>, jsonrpcmsg::Error> {
        Ok(Handled::No(response))
    }

    /// Attempt to claim a notification (`method`/`params`).
    ///
    /// # Important
    ///
    /// The server will not process new messages until this handler returns.
    /// You should avoid blocking in this callback unless you wish to block the server (e.g., for rate limiting).
    /// The recommended approach to manage expensive operations is to use a channel or spawn tasks.
    ///
    /// # Returns
    ///
    /// * `Ok(Handled::Yes)` if the request was claimed.
    /// * `Ok(Handled::No(()))` if not.
    /// * `Err` if an internal error occurs (this will bring down the server).
    #[allow(unused_variables)]
    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &JsonRpcCx,
    ) -> Result<Handled<()>, jsonrpcmsg::Error> {
        Ok(Handled::No(()))
    }
}

/// Return type from JsonRpcHandler; indicates whether the request was handled or not.
#[must_use]
pub enum Handled<T> {
    Yes,
    No(T),
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
    pub fn send_request<Req>(&self, request: Req) -> JsonRpcResponse<Req::Response>
    where
        Req: JsonRpcRequest,
    {
        let (response_tx, response_rx) = oneshot::channel();
        let method = request.method().to_string();
        let result: Result<Option<jsonrpcmsg::Params>, _> = json_cast(request);

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
    pub fn send_notification<N: JsonRpcNotification>(
        &self,
        notification: N,
    ) -> Result<(), jsonrpcmsg::Error> {
        let method = notification.method().to_string();
        let params: Option<jsonrpcmsg::Params> = serde_json::to_value(notification)
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
    pub fn cast<U: serde::Serialize>(self) -> JsonRpcRequestCx<U> {
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
pub trait JsonRpcNotification: serde::de::DeserializeOwned + serde::Serialize {
    /// The method name for the notification.
    fn method(&self) -> &str;
}

///A struct that serializes to the paramcontaining the parameters
pub trait JsonRpcRequest: serde::de::DeserializeOwned + serde::Serialize {
    /// The type of data expected in response.
    type Response: serde::de::DeserializeOwned + serde::Serialize;

    /// The method name for the request.
    fn method(&self) -> &str;
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
