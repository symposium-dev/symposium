//! Core JSON-RPC server support.

use std::fmt::Debug;
use std::ops::Deref;

use boxfnonce::SendBoxFnOnce;
use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::{AsyncRead, AsyncWrite, FutureExt};
use jsonrpcmsg::Params;

mod actors;
mod handlers;
pub use handlers::*;

/// Create a JsonRpcConnection. This can be the basis for either a server or a client.
#[must_use]
pub struct JsonRpcConnection<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> {
    /// Where to send bytes to communicate to the other side
    outgoing_bytes: OB,

    /// Where to read bytes from the other side
    incoming_bytes: IB,

    /// Where the "outgoing messages" actor will receive messages.
    outgoing_rx: mpsc::UnboundedReceiver<OutgoingMessage>,

    /// Sender to send messages to the "outgoing message" actor.
    outgoing_tx: mpsc::UnboundedSender<OutgoingMessage>,

    /// Handler for incoming messages.
    handler: H,
}

impl<OB: AsyncWrite, IB: AsyncRead> JsonRpcConnection<OB, IB, NullHandler> {
    /// Create a new JsonRpcConnection that will read and write from the given streams.
    /// This type follows a builder pattern; use other methods to configure and then invoke
    /// [`Self:serve`] (to use as a server) or [`Self::with_client`] to use as a client.
    pub fn new(outgoing_bytes: OB, incoming_bytes: IB) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
        Self {
            outgoing_bytes,
            incoming_bytes,
            outgoing_rx,
            outgoing_tx,
            handler: NullHandler::default(),
        }
    }
}

impl<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> JsonRpcConnection<OB, IB, H> {
    /// Adds a message handler that will have the opportunity to process incoming messages.
    /// When a new message arrives, handlers are tried in the order they were added, and
    /// the first to "claim" the message "wins".
    pub fn on_receive<H1>(self, handler: H1) -> JsonRpcConnection<OB, IB, ChainHandler<H, H1>>
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
    fn json_rpc_cx(&self) -> JsonRpcConnectionCx {
        JsonRpcConnectionCx::new(self.outgoing_tx.clone())
    }

    /// Runs a server that listens for incoming requests and handles them according to the added handlers.
    pub async fn serve(self) -> Result<(), jsonrpcmsg::Error> {
        let (reply_tx, reply_rx) = mpsc::unbounded();
        let json_rpc_cx = JsonRpcConnectionCx::new(self.outgoing_tx);
        futures::select!(
            r = actors::outgoing_actor(
                self.outgoing_rx,
                reply_tx.clone(),
                self.outgoing_bytes,
            ).fuse() => r?,
            r = actors::incoming_actor(
                &json_rpc_cx,
                self.incoming_bytes,
                reply_tx,
                self.handler,
            ).fuse() => r?,
            r = actors::reply_actor(reply_rx).fuse() => r?,
        );
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
        main_fn: impl AsyncFnOnce(JsonRpcConnectionCx) -> Result<(), jsonrpcmsg::Error>,
    ) -> Result<(), jsonrpcmsg::Error> {
        let cx = self.json_rpc_cx();

        // Run the server + the main function until one terminates.
        // We EXPECT the main function to be the one to terminate
        // except in case of error.
        let result = futures::future::select(Box::pin(self.serve()), Box::pin(main_fn(cx))).await;

        match result {
            Either::Left((Ok(()), _)) => Err(jsonrpcmsg::Error::internal_error()),

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
    /// # Parameters
    ///
    /// * `cx` - The context of the request. This gives access to the request ID and the method name and is used to send a reply; can also be used to send other messages to the other party.
    /// * `params` - The parameters of the request.
    ///
    /// # Returns
    ///
    /// * `Ok(Handled::Yes)` if the request was claimed.
    /// * `Ok(Handled::No(response))` if not.
    /// * `Err` if an internal error occurs (this will bring down the server).
    #[allow(unused_variables)]
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        Ok(Handled::No(cx))
    }

    /// Attempt to claim a notification (`method`/`params`).
    ///
    /// # Important
    ///
    /// The server will not process new messages until this handler returns.
    /// You should avoid blocking in this callback unless you wish to block the server (e.g., for rate limiting).
    /// The recommended approach to manage expensive operations is to use a channel or spawn tasks.
    ///
    /// # Parameters
    ///
    /// * `cx` - The JSON RPC context of the server. Can be used to send messages in response to the notification.
    /// * `params` - The parameters of the request.
    ///
    /// # Returns
    ///
    /// * `Ok(Handled::Yes)` if the request was claimed.
    /// * `Ok(Handled::No(()))` if not.
    /// * `Err` if an internal error occurs (this will bring down the server).
    #[allow(unused_variables)]
    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcNotificationCx>, jsonrpcmsg::Error> {
        Ok(Handled::No(cx))
    }
}

/// Return type from JsonRpcHandler; indicates whether the request was handled or not.
#[must_use]
pub enum Handled<T> {
    Yes,
    No(T),
}

/// Connection context used to send requests/notifications of the other side.
#[derive(Clone)]
pub struct JsonRpcConnectionCx {
    tx: mpsc::UnboundedSender<OutgoingMessage>,
}

impl JsonRpcConnectionCx {
    fn new(tx: mpsc::UnboundedSender<OutgoingMessage>) -> Self {
        Self { tx }
    }

    /// Send an outgoing request and await the reply.
    pub fn send_request<Req: JsonRpcRequest>(
        &self,
        request: Req,
    ) -> JsonRpcResponse<Req::Response> {
        let method = request.method().to_string();
        let (response_tx, response_rx) = oneshot::channel();
        match request.params() {
            Ok(params) => {
                let message = OutgoingMessage::Request {
                    method: method.clone(),
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
            .map(move |json| <Req::Response>::from_value(&method, json))
    }

    /// Send an outgoing notification (no reply expected).)
    pub fn send_notification<N: JsonRpcNotification>(
        &self,
        notification: N,
    ) -> Result<(), jsonrpcmsg::Error> {
        let method = notification.method().to_string();
        let params = notification.params()?;
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
        match &message {
            OutgoingMessage::Response { id, response } => match response {
                Ok(_) => tracing::debug!(?id, "send_raw_message: queuing success response"),
                Err(e) => tracing::warn!(?id, ?e, "send_raw_message: queuing error response"),
            },
            _ => {}
        }
        self.tx
            .unbounded_send(message)
            .map_err(communication_failure)
    }
}

/// The context to respond to an incoming request.
/// Derefs to a [`JsonRpcCx`] which can be used to send other requests and notification.
#[must_use]
pub struct JsonRpcNotificationCx {
    /// The context to use to send outgoing messages and replies.
    cx: JsonRpcConnectionCx,

    /// The method of the request.
    method: String,
}

impl JsonRpcNotificationCx {
    /// Create a new notification context.
    pub fn new(cx: &JsonRpcConnectionCx, method: String) -> Self {
        Self {
            cx: cx.clone(),
            method,
        }
    }

    /// The method of the notification.
    pub fn method(&self) -> &str {
        &self.method
    }
}

impl Deref for JsonRpcNotificationCx {
    type Target = JsonRpcConnectionCx;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}

/// The context to respond to an incoming request.
/// Derefs to a [`JsonRpcCx`] which can be used to send other requests and notification.
#[must_use]
pub struct JsonRpcRequestCx<T: JsonRpcIncomingMessage> {
    /// The context to use to send outgoing messages and replies.
    cx: JsonRpcConnectionCx,

    /// The method of the request.
    method: String,

    /// The `id` of the message we are replying to.
    id: jsonrpcmsg::Id,

    /// Function to send the response `T` to a request with the given method and id.
    make_json: SendBoxFnOnce<
        'static,
        ((String, jsonrpcmsg::Id), Result<T, jsonrpcmsg::Error>),
        Result<serde_json::Value, jsonrpcmsg::Error>,
    >,
}

impl<T: JsonRpcIncomingMessage> std::ops::Deref for JsonRpcRequestCx<T> {
    type Target = JsonRpcConnectionCx;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}

impl JsonRpcRequestCx<serde_json::Value> {
    /// Create a new method context.
    fn new(cx: &JsonRpcConnectionCx, method: String, id: jsonrpcmsg::Id) -> Self {
        Self {
            cx: cx.clone(),
            method,
            id,
            make_json: SendBoxFnOnce::new(move |_, value| value),
        }
    }

    pub fn cast<T: JsonRpcIncomingMessage>(self) -> JsonRpcRequestCx<T> {
        self.wrap_params(move |(method, _), value| match value {
            Ok(value) => T::into_json(value, &method),
            Err(e) => Err(e),
        })
    }
}

impl<T: JsonRpcIncomingMessage> JsonRpcRequestCx<T> {
    /// Get the ID of the request being responded to.
    pub fn id(&self) -> &jsonrpcmsg::Id {
        &self.id
    }

    /// Method of the incoming request
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Convert to a `JsonRpcRequestCx` that expects a JSON value
    /// and which checks (dynamically) that the JSON value it receives
    /// can be converted to `T`.
    pub fn erase_to_json(self) -> JsonRpcRequestCx<serde_json::Value> {
        self.wrap_params(|(method, _id), value| T::from_value(&method, value?))
    }

    /// Return a new JsonRpcResponse that expects a response of type U and serializes it.
    pub fn wrap_method(self, method: String) -> JsonRpcRequestCx<T> {
        JsonRpcRequestCx {
            cx: self.cx,
            method,
            id: self.id,
            make_json: self.make_json,
        }
    }

    /// Return a new JsonRpcResponse that expects a response of type U and serializes it.
    pub fn wrap_params<U: JsonRpcIncomingMessage>(
        self,
        wrap_fn: impl FnOnce(
            (String, jsonrpcmsg::Id),
            Result<U, jsonrpcmsg::Error>,
        ) -> Result<T, jsonrpcmsg::Error>
        + Send
        + 'static,
    ) -> JsonRpcRequestCx<U> {
        JsonRpcRequestCx {
            cx: self.cx,
            method: self.method,
            id: self.id,
            make_json: SendBoxFnOnce::new(
                move |args: (String, jsonrpcmsg::Id), input: Result<U, jsonrpcmsg::Error>| {
                    let t_value = wrap_fn(args.clone(), input);
                    self.make_json.call(args, t_value)
                },
            ),
        }
    }

    /// Get the underlying JSON RPC context.
    pub fn json_rpc_cx(&self) -> JsonRpcConnectionCx {
        self.cx.clone()
    }

    /// Respond to the JSON-RPC request with a value.
    pub fn respond_with_result(
        self,
        response: Result<T, jsonrpcmsg::Error>,
    ) -> Result<(), jsonrpcmsg::Error> {
        match response {
            Ok(r) => {
                tracing::debug!(id = ?self.id, "respond_with_result: Ok, calling respond");
                self.respond(r)
            }
            Err(e) => {
                tracing::debug!(id = ?self.id, ?e, "respond_with_result: Err, calling respond_with_error");
                self.respond_with_error(e)
            }
        }
    }

    /// Respond to the JSON-RPC request with a value.
    pub fn respond_result(
        self,
        response: Result<T, jsonrpcmsg::Error>,
    ) -> Result<(), jsonrpcmsg::Error> {
        tracing::debug!(id = ?self.id, "respond called");
        let json = self
            .make_json
            .call_tuple(((self.method.clone(), self.id.clone()), response));
        self.cx.send_raw_message(OutgoingMessage::Response {
            id: self.id,
            response: json,
        })
    }

    /// Respond to the JSON-RPC request with a value.
    pub fn respond(self, response: T) -> Result<(), jsonrpcmsg::Error> {
        self.respond_result(Ok(response))
    }

    /// Respond to the JSON-RPC request with an internal error.
    pub fn respond_with_internal_error(self) -> Result<(), jsonrpcmsg::Error> {
        self.respond_with_error(jsonrpcmsg::Error::internal_error())
    }

    /// Respond to the JSON-RPC request with an error.
    pub fn respond_with_error(self, error: jsonrpcmsg::Error) -> Result<(), jsonrpcmsg::Error> {
        tracing::debug!(id = ?self.id, ?error, "respond_with_error called");
        self.respond_result(Err(error))
    }
}

/// Common bounds for any JSON-RPC message.
pub trait JsonRpcMessage: 'static + Debug + Sized {}

pub trait JsonRpcIncomingMessage: JsonRpcMessage {
    /// Convert this message into a JSON value.
    fn into_json(self, method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error>;

    /// Parse a JSON value into the response type.
    fn from_value(method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error>;
}

impl JsonRpcMessage for serde_json::Value {}

impl JsonRpcIncomingMessage for serde_json::Value {
    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        Ok(value)
    }

    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        Ok(self)
    }
}

pub trait JsonRpcOutgoingMessage: JsonRpcMessage {
    /// The parameters for the request.
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error>;

    /// The method name for the request.
    fn method(&self) -> &str;
}

/// A struct that represents a notification (JSON-RPC message that does not expect a response).
pub trait JsonRpcNotification: JsonRpcOutgoingMessage {}

/// A struct that represents a request (JSON-RPC message expecting a response).
pub trait JsonRpcRequest: JsonRpcOutgoingMessage {
    /// The type of data expected in response.
    type Response: JsonRpcIncomingMessage;
}

#[derive(Debug)]
pub struct JsonRpcUntypedRequest {
    method: String,
    params: Option<Params>,
}

impl JsonRpcUntypedRequest {
    pub fn new(method: String, params: Option<Params>) -> Self {
        Self { method, params }
    }
}

impl JsonRpcMessage for JsonRpcUntypedRequest {}

impl JsonRpcOutgoingMessage for JsonRpcUntypedRequest {
    fn method(&self) -> &str {
        &self.method
    }

    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        Ok(self.params)
    }
}

impl JsonRpcRequest for JsonRpcUntypedRequest {
    type Response = serde_json::Value;
}

/// Represents a pending response of type `R` from an outgoing request.
pub struct JsonRpcResponse<R> {
    response_rx: oneshot::Receiver<Result<serde_json::Value, jsonrpcmsg::Error>>,
    to_result: Box<dyn Fn(serde_json::Value) -> Result<R, jsonrpcmsg::Error>>,
}

impl JsonRpcResponse<serde_json::Value> {
    fn new(response_rx: oneshot::Receiver<Result<serde_json::Value, jsonrpcmsg::Error>>) -> Self {
        Self {
            response_rx,
            to_result: Box::new(Ok),
        }
    }
}

impl<R: JsonRpcMessage> JsonRpcResponse<R> {
    /// Create a new response that maps the result of the response to a new type.
    pub fn map<U>(
        self,
        map_fn: impl Fn(R) -> Result<U, jsonrpcmsg::Error> + 'static,
    ) -> JsonRpcResponse<U>
    where
        U: JsonRpcMessage,
    {
        JsonRpcResponse {
            response_rx: self.response_rx,
            to_result: Box::new(move |value| map_fn((self.to_result)(value)?)),
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
        (self.to_result)(json_value)
    }
}

const COMMUNICATION_FAILURE: i32 = -32000;

fn communication_failure(err: impl ToString) -> jsonrpcmsg::Error {
    jsonrpcmsg::Error::new(COMMUNICATION_FAILURE, err.to_string())
}
