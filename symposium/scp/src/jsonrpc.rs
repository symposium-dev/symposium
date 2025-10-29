//! Core JSON-RPC server support.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::panic::Location;

use boxfnonce::SendBoxFnOnce;
use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::{AsyncRead, AsyncWrite, FutureExt};

mod actors;
mod handlers;
pub use handlers::*;

use crate::jsonrpc::actors::Task;

/// Create a JsonRpcConnection. This can be the basis for either a server or a client.
#[must_use]
pub struct JsonRpcConnection<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> {
    name: Option<String>,

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
    new_task_rx: mpsc::UnboundedReceiver<Task>,

    /// Sender to send messages to the "new task" actor.
    new_task_tx: mpsc::UnboundedSender<Task>,
}

impl<OB: AsyncWrite, IB: AsyncRead> JsonRpcConnection<OB, IB, NullHandler> {
    /// Create a new JsonRpcConnection that will read and write from the given streams.
    /// This type follows a builder pattern; use other methods to configure and then invoke
    /// [`Self:serve`] (to use as a server) or [`Self::with_client`] to use as a client.
    pub fn new(outgoing_bytes: OB, incoming_bytes: IB) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded();
        let (new_task_tx, new_task_rx) = mpsc::unbounded();
        Self {
            name: None,
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
    /// Set the "name" of this connection -- used only for debugging logs.
    pub fn name(mut self, name: impl ToString) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// Add a new [`JsonRpcHandler`] to the chain.
    ///
    /// Prefer [`Self::on_receive_request`] or [`Self::on_receive_notification`].
    /// This is a low-level method that is not intended for general use.
    pub fn chain_handler<H1: JsonRpcHandler>(
        self,
        handler: H1,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, H1>> {
        JsonRpcConnection {
            name: self.name,
            handler: ChainHandler::new(self.handler, handler),
            outgoing_bytes: self.outgoing_bytes,
            incoming_bytes: self.incoming_bytes,
            outgoing_rx: self.outgoing_rx,
            outgoing_tx: self.outgoing_tx,
            new_task_rx: self.new_task_rx,
            new_task_tx: self.new_task_tx,
        }
    }

    /// Invoke the given closure when either a request or notification is received.
    pub fn on_receive_message<R, N, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, MessageHandler<R, N, F>>>
    where
        R: JsonRpcRequest,
        N: JsonRpcNotification,
        F: AsyncFnMut(MessageAndCx<R, N>) -> Result<(), acp::Error>,
    {
        JsonRpcConnection {
            name: self.name,
            handler: ChainHandler::new(self.handler, MessageHandler::new(op)),
            outgoing_bytes: self.outgoing_bytes,
            incoming_bytes: self.incoming_bytes,
            outgoing_rx: self.outgoing_rx,
            outgoing_tx: self.outgoing_tx,
            new_task_rx: self.new_task_rx,
            new_task_tx: self.new_task_tx,
        }
    }

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
            name: self.name,
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
        F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>,
    {
        JsonRpcConnection {
            name: self.name,
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
    /// **Warning:** This method is provided for use during setup and construction.
    /// It returns a [`JsonRpcConnectionCx`] that you can use later to send requests
    /// or to spawn tasks. But if you do not invoke [`Self::serve`] or [`Self::with_client`],
    /// the server will not be running, and those messages will not be received.
    pub fn json_rpc_cx(&self) -> JsonRpcConnectionCx {
        JsonRpcConnectionCx::new(self.outgoing_tx.clone(), self.new_task_tx.clone())
    }

    /// With a task that will be spawned on the server's executor when it is active.
    #[track_caller]
    pub fn with_spawned(
        self,
        task: impl Future<Output = Result<(), acp::Error>> + Send + 'static,
    ) -> Self {
        let json_rpc_cx = self.json_rpc_cx();
        json_rpc_cx.spawn(task).expect("spawning failed");
        self
    }

    /// Runs a server that listens for incoming requests and handles them according to the added handlers.
    pub async fn serve(self) -> Result<(), acp::Error> {
        let (reply_tx, reply_rx) = mpsc::unbounded();
        let json_rpc_cx = JsonRpcConnectionCx::new(self.outgoing_tx, self.new_task_tx);
        futures::select!(
            r = actors::outgoing_actor(
                &self.name,
                self.outgoing_rx,
                reply_tx.clone(),
                self.outgoing_bytes,
            ).fuse() => {
                tracing::trace!(?r, "outgoing actor terminated");
                r?;
            }
            r = actors::incoming_actor(
                &self.name,
                &json_rpc_cx,
                self.incoming_bytes,
                reply_tx,
                self.handler,
            ).fuse() => {
                tracing::trace!(?r, "incoming actor terminated");
                r?;
            }
            r = actors::reply_actor(reply_rx).fuse() => {
                tracing::trace!(?r, "reply actor terminated");
                r?;
            }
            r = actors::task_actor(self.new_task_rx).fuse() => {
                tracing::trace!(?r, "task actor terminated");
                r?;
            }
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
#[derive(Debug)]
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
    /// Attempt to claim an incoming message (request or notification).
    ///
    /// # Important: do not block
    ///
    /// The server will not process new messages until this handler returns.
    /// You should avoid blocking in this callback unless you wish to block the server (e.g., for rate limiting).
    /// The recommended approach to manage expensive operations is to the [`JsonRpcConnectionCx::spawn`] method available on the message context.
    ///
    /// # Parameters
    ///
    /// * `cx` - The context of the request. This gives access to the request ID and the method name and is used to send a reply; can also be used to send other messages to the other party.
    /// * `params` - The parameters of the request.
    ///
    /// # Returns
    ///
    /// * `Ok(Handled::Yes)` if the message was claimed. It will not be propagated further.
    /// * `Ok(Handled::No(message))` if not; the (possibly changed) message will be passed to the remaining handlers.
    /// * `Err` if an internal error occurs (this will bring down the server).
    async fn handle_message(
        &mut self,
        message: MessageAndCx,
    ) -> Result<Handled<MessageAndCx>, acp::Error>;

    fn describe_chain(&self) -> impl std::fmt::Debug;
}

/// Return type from JsonRpcHandler; indicates whether the request was handled or not.
#[must_use]
pub enum Handled<T> {
    Yes,
    No(T),
}

/// Connection context used to send requests/notifications of the other side.
#[derive(Clone, Debug)]
pub struct JsonRpcConnectionCx {
    message_tx: mpsc::UnboundedSender<OutgoingMessage>,
    task_tx: mpsc::UnboundedSender<Task>,
}

impl JsonRpcConnectionCx {
    fn new(
        tx: mpsc::UnboundedSender<OutgoingMessage>,
        task_tx: mpsc::UnboundedSender<Task>,
    ) -> Self {
        Self {
            message_tx: tx,
            task_tx,
        }
    }

    /// Spawns a task that will run so long as the JSON-RPC connection is being served.
    /// If the task returns an error, the server will shut down.
    #[track_caller]
    pub fn spawn(
        &self,
        future: impl Future<Output = Result<(), acp::Error>> + Send + 'static,
    ) -> Result<(), acp::Error> {
        self.task_tx
            .unbounded_send(Task {
                location: Location::caller(),
                future: Box::pin(future),
            })
            .map_err(acp::Error::into_internal_error)
    }

    /// Send a request/notification and forward the response appropriately.
    ///
    /// The request context's response type matches the request's response type,
    /// enabling type-safe message forwarding.
    pub fn send_proxied_message<R, N>(&self, message: MessageAndCx<R, N>) -> Result<(), acp::Error>
    where
        R: JsonRpcRequest<Response: Send>,
        N: JsonRpcNotification,
    {
        match message {
            MessageAndCx::Request(request, request_cx) => {
                self.send_request(request).forward_to_request_cx(request_cx)
            }
            MessageAndCx::Notification(notification, _) => self.send_notification(notification),
        }
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

        JsonRpcResponse::new(method.clone(), self.clone(), response_rx)
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
        (String, Result<T, acp::Error>),
        Result<serde_json::Value, acp::Error>,
    >,
}

impl<T: JsonRpcResponsePayload> std::fmt::Debug for JsonRpcRequestCx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonRpcRequestCx")
            .field("cx", &self.cx)
            .field("method", &self.method)
            .field("id", &self.id)
            .field("response_type", &std::any::type_name::<T>())
            .finish()
    }
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
            make_json: SendBoxFnOnce::new(move |_method, value| value),
        }
    }

    pub fn cast<T: JsonRpcResponsePayload>(self) -> JsonRpcRequestCx<T> {
        self.wrap_params(move |method, value| match value {
            Ok(value) => T::into_json(value, &method),
            Err(e) => Err(e),
        })
    }
}

impl<T: JsonRpcResponsePayload> JsonRpcRequestCx<T> {
    /// Method of the incoming request
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Convert to a `JsonRpcRequestCx` that expects a JSON value
    /// and which checks (dynamically) that the JSON value it receives
    /// can be converted to `T`.
    pub fn erase_to_json(self) -> JsonRpcRequestCx<serde_json::Value> {
        self.wrap_params(|method, value| T::from_value(&method, value?))
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
    ///
    /// `wrap_fn` will be invoked with the method name and the result of the wrapped function.
    pub fn wrap_params<U: JsonRpcResponsePayload>(
        self,
        wrap_fn: impl FnOnce(&str, Result<U, acp::Error>) -> Result<T, acp::Error> + Send + 'static,
    ) -> JsonRpcRequestCx<U> {
        JsonRpcRequestCx {
            cx: self.cx,
            method: self.method,
            id: self.id,
            make_json: SendBoxFnOnce::new(move |method: String, input: Result<U, acp::Error>| {
                let t_value = wrap_fn(&method, input);
                self.make_json.call(method, t_value)
            }),
        }
    }

    /// Get the underlying JSON RPC context.
    pub fn connection_cx(&self) -> JsonRpcConnectionCx {
        self.cx.clone()
    }

    /// Respond to the JSON-RPC request with either a value (`Ok`) or an error (`Err`).
    pub fn respond_with_result(self, response: Result<T, acp::Error>) -> Result<(), acp::Error> {
        tracing::debug!(id = ?self.id, "respond called");
        let json = self.make_json.call_tuple((self.method.clone(), response));
        self.cx.send_raw_message(OutgoingMessage::Response {
            id: self.id,
            response: json,
        })
    }

    /// Respond to the JSON-RPC request with a value.
    pub fn respond(self, response: T) -> Result<(), acp::Error> {
        self.respond_with_result(Ok(response))
    }

    /// Respond to the JSON-RPC request with an internal error containing a message.
    pub fn respond_with_internal_error(self, message: impl ToString) -> Result<(), acp::Error> {
        self.respond_with_error(crate::util::internal_error(message))
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
    fn parse_request(_method: &str, _params: &impl Serialize) -> Option<Result<Self, acp::Error>>;

    /// Attempt to parse this type from a JSON-RPC notification.
    ///
    /// Returns:
    /// - `None` if this type does not recognize the method name or recognizes it as a request
    /// - `Some(Ok(value))` if the method is recognized as a notification and deserialization succeeds
    /// - `Some(Err(error))` if the method is recognized as a notification but deserialization fails
    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>>;
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

/// An enum capturing an in-flight request or notification.
/// In the case of a request, also includes the context used to respond to the request.
///
/// Type parameters allow specifying the concrete request and notification types.
/// By default, both are `UntypedMessage` for dynamic dispatch.
/// The request context's response type matches the request's response type.
#[derive(Debug)]
pub enum MessageAndCx<R: JsonRpcRequest = UntypedMessage, N: JsonRpcMessage = UntypedMessage> {
    /// Incoming request and the context where the response should be sent.
    Request(R, JsonRpcRequestCx<R::Response>),

    /// Incoming notification.
    Notification(N, JsonRpcConnectionCx),
}

impl<R: JsonRpcRequest, N: JsonRpcMessage> MessageAndCx<R, N> {
    /// Map the request and notification types to new types.
    pub fn map<R1, N1>(
        self,
        map_request: impl FnOnce(
            R,
            JsonRpcRequestCx<R::Response>,
        ) -> (R1, JsonRpcRequestCx<R1::Response>),
        map_notification: impl FnOnce(N, JsonRpcConnectionCx) -> (N1, JsonRpcConnectionCx),
    ) -> MessageAndCx<R1, N1>
    where
        R1: JsonRpcRequest<Response: Send>,
        N1: JsonRpcMessage,
    {
        match self {
            MessageAndCx::Request(request, cx) => {
                let (new_request, new_cx) = map_request(request, cx);
                MessageAndCx::Request(new_request, new_cx)
            }
            MessageAndCx::Notification(notification, cx) => {
                let (new_notification, new_cx) = map_notification(notification, cx);
                MessageAndCx::Notification(new_notification, new_cx)
            }
        }
    }

    /// Respond to the message with an error.
    ///
    /// If this message is a request, this error becomes the reply to the request.
    ///
    /// If this message is a notification, the error is sent as a notification.
    pub fn respond_with_error(self, error: acp::Error) -> Result<(), acp::Error> {
        match self {
            MessageAndCx::Request(_, cx) => cx.respond_with_error(error),
            MessageAndCx::Notification(_, cx) => cx.send_error_notification(error),
        }
    }
}

impl MessageAndCx {
    /// Returns the method of the message (only available for UntypedMessage).
    pub fn method(&self) -> &str {
        match self {
            MessageAndCx::Request(msg, _) => &msg.method,
            MessageAndCx::Notification(msg, _) => &msg.method,
        }
    }

    /// Returns the message of the message (only available for UntypedMessage).
    pub fn message(&self) -> &UntypedMessage {
        match self {
            MessageAndCx::Request(msg, _) => msg,
            MessageAndCx::Notification(msg, _) => msg,
        }
    }
}

/// An incoming JSON message without any typing. Can be a request or a notification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        Some(UntypedMessage::new(method, params))
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        Some(UntypedMessage::new(method, params))
    }
}

impl JsonRpcRequest for UntypedMessage {
    type Response = serde_json::Value;
}

impl JsonRpcNotification for UntypedMessage {}

/// Represents a pending response of type `R` from an outgoing request.
pub struct JsonRpcResponse<R> {
    method: String,
    connection_cx: JsonRpcConnectionCx,
    response_rx: oneshot::Receiver<Result<serde_json::Value, acp::Error>>,
    to_result: Box<dyn Fn(serde_json::Value) -> Result<R, acp::Error> + Send>,
}

impl JsonRpcResponse<serde_json::Value> {
    fn new(
        method: String,
        connection_cx: JsonRpcConnectionCx,
        response_rx: oneshot::Receiver<Result<serde_json::Value, acp::Error>>,
    ) -> Self {
        Self {
            method,
            response_rx,
            connection_cx,
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
            connection_cx: self.connection_cx,
            to_result: Box::new(move |value| map_fn((self.to_result)(value)?)),
        }
    }

    /// Schedule an async task that will forward the respond to `response_cx` when it arrives.
    /// Useful when proxying messages around.
    pub fn forward_to_request_cx(self, request_cx: JsonRpcRequestCx<R>) -> Result<(), acp::Error>
    where
        R: Send,
    {
        self.await_when_result_received(async move |result| request_cx.respond_with_result(result))
    }

    /// Block the current task until the response is received.
    ///
    /// This is useful in spawned tasks. It should *not* be used
    /// in callbacks like [`JsonRpcConnection::on_receive_message`]
    /// as that will prevent the event loop from running.
    ///
    /// In a callback setting, prefer [`Self::await_when_response_received`].
    pub async fn block_task(self) -> Result<R, acp::Error>
    where
        R: Send,
    {
        match self.response_rx.await {
            Ok(Ok(json_value)) => match (self.to_result)(json_value) {
                Ok(value) => Ok(value),
                Err(err) => Err(err),
            },
            Ok(Err(err)) => Err(err),
            Err(err) => Err(crate::util::internal_error(format!(
                "response to `{}` never received: {}",
                self.method, err
            ))),
        }
    }

    /// Schedule an async task to run when a successful response is received.
    /// If an error occurs, that error will be returned to `request_cx` as the result.
    #[track_caller]
    pub fn await_when_ok_response_received<F>(
        self,
        request_cx: JsonRpcRequestCx<R>,
        task: impl FnOnce(R, JsonRpcRequestCx<R>) -> F + 'static + Send,
    ) -> Result<(), acp::Error>
    where
        F: Future<Output = Result<(), acp::Error>> + 'static + Send,
        R: Send,
    {
        self.await_when_result_received(async move |result| match result {
            Ok(value) => task(value, request_cx).await,
            Err(err) => request_cx.respond_with_error(err),
        })
    }

    /// Schedule an async task to run when the response is received.
    ///
    /// It is intentionally not possible to block until the response is received
    /// because doing so can easily stall the event loop if done directly in the `on_receive` callback.
    ///
    /// If this task ultimately returns `Err`, the server will abort.
    #[track_caller]
    pub fn await_when_result_received<F>(
        self,
        task: impl FnOnce(Result<R, acp::Error>) -> F + 'static + Send,
    ) -> Result<(), acp::Error>
    where
        F: Future<Output = Result<(), acp::Error>> + 'static + Send,
        R: Send,
    {
        let connection_cx = self.connection_cx.clone();
        let block_task = self.block_task();
        connection_cx.spawn(async move { task(block_task.await).await })
    }
}

const COMMUNICATION_FAILURE: i32 = -32000;

fn communication_failure(err: impl ToString) -> acp::Error {
    acp::Error::new((COMMUNICATION_FAILURE, err.to_string()))
}
