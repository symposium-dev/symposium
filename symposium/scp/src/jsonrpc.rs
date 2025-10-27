//! Core JSON-RPC server support.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::ops::Deref;
use tracing::Instrument as _;

use boxfnonce::SendBoxFnOnce;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Either};
use futures::{AsyncRead, AsyncWrite, FutureExt};

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

    /// Receiver for new tasks.
    new_task_rx: mpsc::UnboundedReceiver<BoxFuture<'static, Result<(), acp::Error>>>,

    /// Sender to send messages to the "new task" actor.
    new_task_tx: mpsc::UnboundedSender<BoxFuture<'static, Result<(), acp::Error>>>,
}

impl<OB: AsyncWrite, IB: AsyncRead> JsonRpcConnection<OB, IB, NullHandler> {
    /// Create a new JsonRpcConnection that will read and write from the given streams.
    /// This type follows a builder pattern; use other methods to configure and then invoke
    /// [`Self:serve`] (to use as a server) or [`Self::with_client`] to use as a client.
    pub fn new(outgoing_bytes: OB, incoming_bytes: IB) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
        let (new_task_tx, new_task_rx) = mpsc::unbounded();
        Self {
            outgoing_bytes,
            incoming_bytes,
            outgoing_rx,
            outgoing_tx,
            handler: NullHandler::default(),
            new_task_rx,
            new_task_tx,
        }
    }
}

impl<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> JsonRpcConnection<OB, IB, H> {
    /// Invoke the given closure when a request is received.
    pub fn on_receive_request<R, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, RequestHandler<R, F>>>
    where
        R: JsonRpcRequest,
        F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
    {
        JsonRpcConnection {
            handler: ChainHandler::new(self.handler, RequestHandler::new(op)),
            outgoing_bytes: self.outgoing_bytes,
            incoming_bytes: self.incoming_bytes,
            outgoing_rx: self.outgoing_rx,
            outgoing_tx: self.outgoing_tx,
            new_task_rx: self.new_task_rx,
            new_task_tx: self.new_task_tx,
        }
    }

    /// Invoke the given closure when a notification is received.
    pub fn on_receive_notification<N, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, NotificationHandler<N, F>>>
    where
        N: JsonRpcNotification,
        F: AsyncFnMut(N, JsonRpcNotificationCx) -> Result<(), acp::Error>,
    {
        JsonRpcConnection {
            handler: ChainHandler::new(self.handler, NotificationHandler::new(op)),
            outgoing_bytes: self.outgoing_bytes,
            incoming_bytes: self.incoming_bytes,
            outgoing_rx: self.outgoing_rx,
            outgoing_tx: self.outgoing_tx,
            new_task_rx: self.new_task_rx,
            new_task_tx: self.new_task_tx,
        }
    }

    /// Returns a [`JsonRpcCx`] that allows you to send requests over the connection
    /// and receive responses.
    ///
    /// This is private because it would give people a footgun if they had it,
    /// since they might try to use it when the server is not running and deadlock,
    /// and I don't really think they need it.
    fn json_rpc_cx(&self) -> JsonRpcConnectionCx {
        JsonRpcConnectionCx::new(self.outgoing_tx.clone(), self.new_task_tx.clone())
    }

    /// Runs a server that listens for incoming requests and handles them according to the added handlers.
    pub async fn serve(self) -> Result<(), acp::Error> {
        let (reply_tx, reply_rx) = mpsc::unbounded();
        let json_rpc_cx = JsonRpcConnectionCx::new(self.outgoing_tx, self.new_task_tx);
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
            r = actors::task_actor(self.new_task_rx).fuse() => r?,
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
        main_fn: impl AsyncFnOnce(JsonRpcConnectionCx) -> Result<(), acp::Error>,
    ) -> Result<(), acp::Error> {
        let cx = self.json_rpc_cx();

        // Run the server + the main function until one terminates.
        // We EXPECT the main function to be the one to terminate
        // except in case of error.
        let result = futures::future::select(Box::pin(self.serve()), Box::pin(main_fn(cx))).await;

        match result {
            Either::Left((Ok(()), _)) => Err(acp::Error::internal_error()),

            Either::Left((result, _)) | Either::Right((result, _)) => result,
        }
    }
}

/// Message sent to the reply management actor
enum ReplyMessage {
    /// Wait for a response to the given id and then send it to the given receiver
    Subscribe(
        jsonrpcmsg::Id,
        oneshot::Sender<Result<serde_json::Value, acp::Error>>,
    ),

    /// Dispatch a response to the given id and value
    Dispatch(jsonrpcmsg::Id, Result<serde_json::Value, acp::Error>),
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
        response_tx: oneshot::Sender<Result<serde_json::Value, acp::Error>>,
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

        response: Result<serde_json::Value, acp::Error>,
    },

    /// Send a generalized error message
    Error { error: acp::Error },
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
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, acp::Error> {
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
    ) -> Result<Handled<JsonRpcNotificationCx>, acp::Error> {
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
    message_tx: mpsc::UnboundedSender<OutgoingMessage>,
    task_tx: mpsc::UnboundedSender<BoxFuture<'static, Result<(), acp::Error>>>,
}

impl JsonRpcConnectionCx {
    fn new(
        tx: mpsc::UnboundedSender<OutgoingMessage>,
        task_tx: mpsc::UnboundedSender<BoxFuture<'static, Result<(), acp::Error>>>,
    ) -> Self {
        Self {
            message_tx: tx,
            task_tx,
        }
    }

    /// Spawns a task that will run so long as the JSON-RPC connection is being served.
    /// If the task returns an error, the server will shut down.
    pub fn spawn(
        &self,
        future: impl Future<Output = Result<(), acp::Error>> + Send + 'static,
    ) -> Result<(), acp::Error> {
        self.task_tx
            .unbounded_send(Box::pin(future))
            .map_err(acp::Error::into_internal_error)
    }

    /// Send an outgoing request and await the reply.
    pub fn send_request<Req: JsonRpcRequest>(
        &self,
        request: Req,
    ) -> JsonRpcResponse<Req::Response> {
        let method = request.method().to_string();
        let (response_tx, response_rx) = oneshot::channel();
        match request.into_untyped_message() {
            Ok(untyped) => {
                let params = crate::util::json_cast(untyped.params).ok();
                let message = OutgoingMessage::Request {
                    method: method.clone(),
                    params,
                    response_tx,
                };

                match self.message_tx.unbounded_send(message) {
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

        JsonRpcResponse::new(method.clone(), response_rx, self.task_tx.clone())
            .map(move |json| <Req::Response>::from_value(&method, json))
    }

    /// Send an outgoing notification (no reply expected).)
    pub fn send_notification<N: JsonRpcNotification>(
        &self,
        notification: N,
    ) -> Result<(), acp::Error> {
        let untyped = notification.into_untyped_message()?;
        let params = crate::util::json_cast(untyped.params).ok();
        self.send_raw_message(OutgoingMessage::Notification {
            method: untyped.method,
            params,
        })
    }

    /// Send an error notification (no reply expected).
    pub fn send_error_notification(&self, error: acp::Error) -> Result<(), acp::Error> {
        self.send_raw_message(OutgoingMessage::Error { error })
    }

    fn send_raw_message(&self, message: OutgoingMessage) -> Result<(), acp::Error> {
        match &message {
            OutgoingMessage::Response { id, response } => match response {
                Ok(_) => tracing::debug!(?id, "send_raw_message: queuing success response"),
                Err(e) => tracing::warn!(?id, ?e, "send_raw_message: queuing error response"),
            },
            _ => {}
        }
        self.message_tx
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
pub struct JsonRpcRequestCx<T: JsonRpcResponsePayload> {
    /// The context to use to send outgoing messages and replies.
    cx: JsonRpcConnectionCx,

    /// The method of the request.
    method: String,

    /// The `id` of the message we are replying to.
    id: jsonrpcmsg::Id,

    /// Function to send the response `T` to a request with the given method and id.
    make_json: SendBoxFnOnce<
        'static,
        ((String, jsonrpcmsg::Id), Result<T, acp::Error>),
        Result<serde_json::Value, acp::Error>,
    >,
}

impl<T: JsonRpcResponsePayload> std::ops::Deref for JsonRpcRequestCx<T> {
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

    pub fn cast<T: JsonRpcResponsePayload>(self) -> JsonRpcRequestCx<T> {
        self.wrap_params(move |(method, _), value| match value {
            Ok(value) => T::into_json(value, &method),
            Err(e) => Err(e),
        })
    }
}

impl<T: JsonRpcResponsePayload> JsonRpcRequestCx<T> {
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
    pub fn wrap_params<U: JsonRpcResponsePayload>(
        self,
        wrap_fn: impl FnOnce((String, jsonrpcmsg::Id), Result<U, acp::Error>) -> Result<T, acp::Error>
        + Send
        + 'static,
    ) -> JsonRpcRequestCx<U> {
        JsonRpcRequestCx {
            cx: self.cx,
            method: self.method,
            id: self.id,
            make_json: SendBoxFnOnce::new(
                move |args: (String, jsonrpcmsg::Id), input: Result<U, acp::Error>| {
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

    /// Respond to the JSON-RPC request with either a value (`Ok`) or an error (`Err`).
    pub fn respond_with_result(self, response: Result<T, acp::Error>) -> Result<(), acp::Error> {
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
    pub fn respond(self, response: T) -> Result<(), acp::Error> {
        self.respond_with_result(Ok(response))
    }

    /// Respond to the JSON-RPC request with an internal error.
    pub fn respond_with_internal_error(self) -> Result<(), acp::Error> {
        self.respond_with_error(acp::Error::internal_error())
    }

    /// Respond to the JSON-RPC request with an error.
    pub fn respond_with_error(self, error: acp::Error) -> Result<(), acp::Error> {
        tracing::debug!(id = ?self.id, ?error, "respond_with_error called");
        self.respond_with_result(Err(error))
    }
}

/// Common bounds for any JSON-RPC message.
pub trait JsonRpcMessage: 'static + Debug + Sized {
    /// The parameters for the request.
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error>;

    /// The method name for the request.
    fn method(&self) -> &str;

    /// Attempt to parse this type from a JSON-RPC request.
    ///
    /// Returns:
    /// - `None` if this type does not recognize the method name or recognizes it as a notification
    /// - `Some(Ok(value))` if the method is recognized as a request and deserialization succeeds
    /// - `Some(Err(error))` if the method is recognized as a request but deserialization fails
    fn parse_request(
        _method: &str,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Option<Result<Self, acp::Error>> {
        None
    }

    /// Attempt to parse this type from a JSON-RPC notification.
    ///
    /// Returns:
    /// - `None` if this type does not recognize the method name or recognizes it as a request
    /// - `Some(Ok(value))` if the method is recognized as a notification and deserialization succeeds
    /// - `Some(Err(error))` if the method is recognized as a notification but deserialization fails
    fn parse_notification(
        _method: &str,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Option<Result<Self, acp::Error>> {
        None
    }
}

/// Defines the "payload" of a successful response to a JSON-RPC request.
pub trait JsonRpcResponsePayload: 'static + Debug + Sized {
    /// Convert this message into a JSON value.
    fn into_json(self, method: &str) -> Result<serde_json::Value, acp::Error>;

    /// Parse a JSON value into the response type.
    fn from_value(method: &str, value: serde_json::Value) -> Result<Self, acp::Error>;
}

impl JsonRpcResponsePayload for serde_json::Value {
    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        Ok(value)
    }

    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        Ok(self)
    }
}

/// A struct that represents a notification (JSON-RPC message that does not expect a response).
pub trait JsonRpcNotification: JsonRpcMessage {}

/// A struct that represents a request (JSON-RPC message expecting a response).
pub trait JsonRpcRequest: JsonRpcMessage {
    /// The type of data expected in response.
    type Response: JsonRpcResponsePayload;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UntypedMessage {
    pub method: String,
    pub params: serde_json::Value,
}

impl UntypedMessage {
    /// Returns an untyped message with the given method and parameters.
    pub fn new(method: &str, params: impl Serialize) -> Result<Self, acp::Error> {
        let params = serde_json::to_value(params)?;
        Ok(Self {
            method: method.to_string(),
            params,
        })
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn params(&self) -> &serde_json::Value {
        &self.params
    }

    pub fn into_parts(self) -> (String, serde_json::Value) {
        (self.method, self.params)
    }
}

impl JsonRpcMessage for UntypedMessage {
    fn method(&self) -> &str {
        &self.method
    }

    fn into_untyped_message(self) -> Result<UntypedMessage, agent_client_protocol::Error> {
        Ok(self)
    }
}

impl JsonRpcRequest for UntypedMessage {
    type Response = serde_json::Value;
}

impl JsonRpcNotification for UntypedMessage {}

/// Represents a pending response of type `R` from an outgoing request.
pub struct JsonRpcResponse<R> {
    method: String,
    response_rx: oneshot::Receiver<Result<serde_json::Value, acp::Error>>,
    task_tx: mpsc::UnboundedSender<BoxFuture<'static, Result<(), acp::Error>>>,
    to_result: Box<dyn Fn(serde_json::Value) -> Result<R, acp::Error> + Send>,
}

impl JsonRpcResponse<serde_json::Value> {
    fn new(
        method: String,
        response_rx: oneshot::Receiver<Result<serde_json::Value, acp::Error>>,
        task_tx: mpsc::UnboundedSender<BoxFuture<'static, Result<(), acp::Error>>>,
    ) -> Self {
        Self {
            method,
            response_rx,
            task_tx,
            to_result: Box::new(Ok),
        }
    }
}

impl<R: JsonRpcResponsePayload> JsonRpcResponse<R> {
    /// Create a new response that maps the result of the response to a new type.
    pub fn map<U>(
        self,
        map_fn: impl Fn(R) -> Result<U, acp::Error> + 'static + Send,
    ) -> JsonRpcResponse<U>
    where
        U: JsonRpcResponsePayload,
    {
        JsonRpcResponse {
            method: self.method,
            response_rx: self.response_rx,
            task_tx: self.task_tx,
            to_result: Box::new(move |value| map_fn((self.to_result)(value)?)),
        }
    }

    /// Schedule an async task that will forward the respond to `response_cx` when it arrives.
    /// Useful when proxying messages around.
    pub fn forward_to_request_cx(self, request_cx: JsonRpcRequestCx<R>) -> Result<(), acp::Error>
    where
        R: Send,
    {
        self.await_when_response_received(async move |result| {
            request_cx.respond_with_result(result)
        })
    }

    /// Schedule an async task to run when the response is received.
    ///
    /// It is intentionally not possible to block until the response is received
    /// because doing so can easily stall the event loop if done directly in the `on_receive` callback.
    ///
    /// If this task ultimately returns `Err`, the server will abort.
    pub fn await_when_response_received<F>(
        self,
        task: impl FnOnce(Result<R, acp::Error>) -> F + 'static + Send,
    ) -> Result<(), acp::Error>
    where
        F: Future<Output = Result<(), acp::Error>> + 'static + Send,
    {
        let current_span = tracing::Span::current();
        self.task_tx
            .unbounded_send(Box::pin(
                async move {
                    let result = match self.response_rx.await {
                        // We received a JSON value; transform it to our result type
                        Ok(Ok(json_value)) => {
                            tracing::trace!(?json_value, "received response to message");
                            (self.to_result)(json_value)
                        }

                        // We got sent an error
                        Ok(Err(e)) => Err(e),

                        // if the `response_tx` is dropped before we get a chance, that's weird
                        Err(e) => Err(acp::Error::new((
                            COMMUNICATION_FAILURE,
                            format!(
                                "reply of type `{}` never arrived: {e}",
                                std::any::type_name::<R>()
                            ),
                        ))),
                    };
                    task(result).await
                }
                .instrument(tracing::info_span!(
                    "receive_response",
                    method = self.method
                ))
                .instrument(current_span),
            ))
            .map_err(acp::Error::into_internal_error)
    }
}

const COMMUNICATION_FAILURE: i32 = -32000;

fn communication_failure(err: impl ToString) -> acp::Error {
    acp::Error::new((COMMUNICATION_FAILURE, err.to_string()))
}
