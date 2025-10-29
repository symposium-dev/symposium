use agent_client_protocol::{self as acp, InitializeRequest, InitializeResponse};
use futures::{AsyncRead, AsyncWrite};
use scp::{
    ChainHandler, Handled, JsonRpcConnection, JsonRpcConnectionCx, JsonRpcHandler, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcRequestCx, MessageAndCx, MetaCapabilityExt, Proxy,
    UntypedMessage,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

use crate::mcp_server::McpServiceRegistry;

// Requests and notifications send between us and the successor
// ============================================================

const SUCCESSOR_REQUEST_METHOD: &str = "_proxy/successor/request";

/// A request being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a request downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessorRequest<Req: JsonRpcRequest> {
    /// The message to be sent to the successor component.
    #[serde(flatten)]
    pub request: Req,
}

impl<Req: JsonRpcRequest> JsonRpcMessage for SuccessorRequest<Req> {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, acp::Error> {
        scp::UntypedMessage::new(
            SUCCESSOR_REQUEST_METHOD,
            SuccessorRequest {
                request: self.request.into_untyped_message()?,
            },
        )
    }

    fn method(&self) -> &str {
        SUCCESSOR_REQUEST_METHOD
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method == SUCCESSOR_REQUEST_METHOD {
            match scp::util::json_cast::<_, SuccessorRequest<scp::UntypedMessage>>(params) {
                Ok(outer) => match Req::parse_request(&outer.request.method, &outer.request.params)
                {
                    Some(Ok(request)) => Some(Ok(SuccessorRequest { request })),
                    Some(Err(err)) => Some(Err(err)),
                    None => None,
                },
                Err(err) => Some(Err(err)),
            }
        } else {
            None
        }
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        None // Request, not notification
    }
}

impl<Req: JsonRpcRequest> JsonRpcRequest for SuccessorRequest<Req> {
    type Response = Req::Response;
}

const SUCCESSOR_NOTIFICATION_METHOD: &str = "_proxy/successor/notification";

/// A notification being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a notification downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessorNotification<Req: JsonRpcNotification> {
    /// The message to be sent to the successor component.
    #[serde(flatten)]
    pub notification: Req,
}

impl<Req: JsonRpcNotification> JsonRpcMessage for SuccessorNotification<Req> {
    fn into_untyped_message(self) -> Result<scp::UntypedMessage, acp::Error> {
        scp::UntypedMessage::new(
            SUCCESSOR_NOTIFICATION_METHOD,
            SuccessorNotification {
                notification: self.notification.into_untyped_message()?,
            },
        )
    }

    fn method(&self) -> &str {
        SUCCESSOR_NOTIFICATION_METHOD
    }

    fn parse_request(_method: &str, _params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        None // Notification, not request
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        if method == SUCCESSOR_NOTIFICATION_METHOD {
            match scp::util::json_cast::<_, SuccessorNotification<scp::UntypedMessage>>(params) {
                Ok(outer) => match Req::parse_notification(
                    &outer.notification.method,
                    &outer.notification.params,
                ) {
                    Some(Ok(notification)) => Some(Ok(SuccessorNotification { notification })),
                    Some(Err(err)) => Some(Err(err)),
                    None => None,
                },
                Err(err) => Some(Err(err)),
            }
        } else {
            None
        }
    }
}

impl<Req: JsonRpcNotification> JsonRpcNotification for SuccessorNotification<Req> {}

// Proxy methods
// ============================================================

pub trait AcpProxyExt<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> {
    /// Adds a handler for requests received from the successor component.
    ///
    /// The provided handler will receive unwrapped ACP messages - the
    /// `_proxy/successor/receive/*` protocol wrappers are handled automatically.
    /// Your handler processes normal ACP requests and notifications as if it were
    /// a regular ACP component.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use scp::proxy::JsonRpcConnectionExt;
    /// # use scp::{JsonRpcConnection, JsonRpcHandler};
    /// # struct MyHandler;
    /// # impl JsonRpcHandler for MyHandler {}
    /// # async fn example() -> Result<(), acp::Error> {
    /// JsonRpcConnection::new(tokio::io::stdin(), tokio::io::stdout())
    ///     .on_receive_from_successor(MyHandler)
    ///     .serve()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    fn on_receive_request_from_successor<R, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, RequestFromSuccessorHandler<R, F>>>
    where
        R: JsonRpcRequest,
        F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>;

    /// Adds a handler for messages received from the successor component.
    ///
    /// The provided handler will receive unwrapped ACP messages - the
    /// `_proxy/successor/receive/*` protocol wrappers are handled automatically.
    /// Your handler processes normal ACP requests and notifications as if it were
    /// a regular ACP component.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use scp::proxy::JsonRpcConnectionExt;
    /// # use scp::{JsonRpcConnection, JsonRpcHandler};
    /// # struct MyHandler;
    /// # impl JsonRpcHandler for MyHandler {}
    /// # async fn example() -> Result<(), acp::Error> {
    /// JsonRpcConnection::new(tokio::io::stdin(), tokio::io::stdout())
    ///     .on_receive_from_successor(MyHandler)
    ///     .serve()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    fn on_receive_notification_from_successor<N, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, NotificationFromSuccessorHandler<N, F>>>
    where
        N: JsonRpcNotification,
        F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>;

    /// Installs a proxy layer that proxies all requests/notifications to/from the successor.
    /// This is typically the last component in the chain.
    fn proxy(self) -> JsonRpcConnection<OB, IB, ChainHandler<H, ProxyHandler>>;

    /// Provide MCP servers to downstream successors.
    /// This layer will modify `session/new` requests to include those MCP servers
    /// (unless you intercept them earlier).
    fn provide_mcp(
        self,
        registry: impl AsRef<McpServiceRegistry>,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, McpServiceRegistry>>;
}

impl<OB, IB, H> AcpProxyExt<OB, IB, H> for JsonRpcConnection<OB, IB, H>
where
    OB: AsyncWrite,
    IB: AsyncRead,
    H: JsonRpcHandler,
{
    fn on_receive_request_from_successor<R, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, RequestFromSuccessorHandler<R, F>>>
    where
        R: JsonRpcRequest,
        F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
    {
        self.chain_handler(RequestFromSuccessorHandler::new(op))
    }

    fn on_receive_notification_from_successor<N, F>(
        self,
        op: F,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, NotificationFromSuccessorHandler<N, F>>>
    where
        N: JsonRpcNotification,
        F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>,
    {
        self.chain_handler(NotificationFromSuccessorHandler::new(op))
    }

    fn proxy(self) -> JsonRpcConnection<OB, IB, ChainHandler<H, ProxyHandler>> {
        self.chain_handler(ProxyHandler {})
    }

    fn provide_mcp(
        self,
        registry: impl AsRef<McpServiceRegistry>,
    ) -> JsonRpcConnection<OB, IB, ChainHandler<H, McpServiceRegistry>> {
        self.chain_handler(registry.as_ref().clone())
    }
}

/// Handler to process a request of type `R` coming from the successor component.
pub struct RequestFromSuccessorHandler<R, F>
where
    R: JsonRpcRequest,
    F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
{
    handler: F,
    phantom: PhantomData<fn(R)>,
}

impl<R, F> RequestFromSuccessorHandler<R, F>
where
    R: JsonRpcRequest,
    F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
{
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            phantom: PhantomData,
        }
    }
}

impl<R, F> JsonRpcHandler for RequestFromSuccessorHandler<R, F>
where
    R: JsonRpcRequest,
    F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
{
    async fn handle_message(
        &mut self,
        message: scp::MessageAndCx,
    ) -> Result<Handled<scp::MessageAndCx>, agent_client_protocol::Error> {
        let MessageAndCx::Request(request, cx) = message else {
            return Ok(Handled::No(message));
        };

        tracing::debug!(
            request_type = std::any::type_name::<R>(),
            message = ?request,
            "RequestHandler::handle_request"
        );
        match <SuccessorRequest<R>>::parse_request(&request.method, &request.params) {
            Some(Ok(request)) => {
                tracing::trace!(?request, "RequestHandler::handle_request: parse completed");
                (self.handler)(request.request, cx.cast()).await?;
                Ok(Handled::Yes)
            }
            Some(Err(err)) => {
                tracing::trace!(?err, "RequestHandler::handle_request: parse errored");
                Err(err)
            }
            None => {
                tracing::trace!("RequestHandler::handle_request: parse failed");
                Ok(Handled::No(MessageAndCx::Request(request, cx)))
            }
        }
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        std::any::type_name::<R>()
    }
}

/// Handler to process a notification of type `N` coming from the successor component.
pub struct NotificationFromSuccessorHandler<N, F>
where
    N: JsonRpcNotification,
    F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>,
{
    handler: F,
    phantom: PhantomData<fn(N)>,
}

impl<N, F> NotificationFromSuccessorHandler<N, F>
where
    N: JsonRpcNotification,
    F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>,
{
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            phantom: PhantomData,
        }
    }
}

impl<N, F> JsonRpcHandler for NotificationFromSuccessorHandler<N, F>
where
    N: JsonRpcNotification,
    F: AsyncFnMut(N, JsonRpcConnectionCx) -> Result<(), acp::Error>,
{
    async fn handle_message(
        &mut self,
        message: scp::MessageAndCx,
    ) -> Result<Handled<scp::MessageAndCx>, agent_client_protocol::Error> {
        let MessageAndCx::Notification(message, cx) = message else {
            return Ok(Handled::No(message));
        };

        match <SuccessorNotification<N>>::parse_notification(&message.method, &message.params) {
            Some(Ok(notification)) => {
                tracing::trace!(
                    ?notification,
                    "NotificationFromSuccessorHandler::handle_request: parse completed"
                );
                (self.handler)(notification.notification, cx).await?;
                Ok(Handled::Yes)
            }
            Some(Err(err)) => {
                tracing::trace!(
                    ?err,
                    "NotificationFromSuccessorHandler::handle_request: parse errored"
                );
                Err(err)
            }
            None => {
                tracing::trace!("NotificationFromSuccessorHandler::handle_request: parse failed");
                Ok(Handled::No(MessageAndCx::Notification(message, cx)))
            }
        }
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        format!("FromSuccessor<{}>", std::any::type_name::<N>())
    }
}

/// Handler for the "default proxy" behavior.
pub struct ProxyHandler {}

impl JsonRpcHandler for ProxyHandler {
    fn describe_chain(&self) -> impl std::fmt::Debug {
        "proxy"
    }

    async fn handle_message(
        &mut self,
        message: scp::MessageAndCx,
    ) -> Result<Handled<scp::MessageAndCx>, agent_client_protocol::Error> {
        tracing::debug!(
            message = ?message.message(),
            "ProxyHandler::handle_request"
        );

        match message {
            MessageAndCx::Request(request, request_cx) => {
                // If we receive a request from the successor, send it to our predecessor.
                if let Some(result) = <SuccessorRequest<UntypedMessage>>::parse_request(
                    &request.method,
                    &request.params,
                ) {
                    let request = result?;
                    request_cx
                        .send_request(request.request)
                        .forward_to_request_cx(request_cx)?;
                    return Ok(Handled::Yes);
                }

                // If we receive "Initialize", require the proxy capability (and remove it)
                if let Some(result) =
                    InitializeRequest::parse_request(&request.method, &request.params)
                {
                    let request = result?;
                    return self
                        .forward_initialize(request, request_cx.cast())
                        .await
                        .map(|()| Handled::Yes);
                }

                // If we receive any other request, send it to our successor.
                request_cx
                    .send_request_to_successor(request)
                    .forward_to_request_cx(request_cx)?;
                Ok(Handled::Yes)
            }

            MessageAndCx::Notification(notification, cx) => {
                // If we receive a request from the successor, send it to our predecessor.
                if let Some(result) = <SuccessorNotification<UntypedMessage>>::parse_notification(
                    &notification.method,
                    &notification.params,
                ) {
                    match result {
                        Ok(r) => {
                            cx.send_notification(r.notification)?;
                            return Ok(Handled::Yes);
                        }
                        Err(err) => return Err(err),
                    }
                }

                // If we receive any other request, send it to our successor.
                cx.send_notification_to_successor(notification)?;
                Ok(Handled::Yes)
            }
        }
    }
}

impl ProxyHandler {
    /// Proxy initialization requires (1) a `Proxy` capability to be
    /// provided by the conductor and (2) provides a `Proxy` capability
    /// in our response.
    async fn forward_initialize(
        &mut self,
        mut request: InitializeRequest,
        request_cx: JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        tracing::debug!(
            method = request_cx.method(),
            params = ?request,
            "ProxyHandler::forward_initialize"
        );

        if !request.has_meta_capability(Proxy) {
            request_cx.respond_with_error(
                acp::Error::invalid_params()
                    .with_data("this command requires the proxy capability"),
            )?;
            return Ok(());
        }

        request = request.remove_meta_capability(Proxy);
        request_cx
            .send_request_to_successor(request)
            .await_when_result_received(async move |mut result| {
                result = result.map(|r| r.add_meta_capability(Proxy));
                request_cx.respond_with_result(result)
            })
    }
}

/// Extension trait for [`JsonRpcCx`] that adds methods for sending to successor.
///
/// This trait provides convenient methods for proxies to forward messages downstream
/// to their successor component (next proxy or agent). Messages are automatically
/// wrapped in the `_proxy/successor/send/*` protocol format.
///
/// # Example
///
/// ```rust,ignore
/// // Example using ACP request types
/// use scp::proxy::JsonRpcCxExt;
/// use agent_client_protocol_schema::agent::PromptRequest;
///
/// async fn forward_prompt(cx: &JsonRpcCx, prompt: PromptRequest) {
///     let response = cx.send_request_to_successor(prompt).recv().await?;
///     // response is the typed response from the successor
/// }
/// ```
pub trait JsonRpcCxExt {
    /// Send a request to the successor component.
    ///
    /// The request is automatically wrapped in a `ToSuccessorRequest` and sent
    /// using the `_proxy/successor/send/request` method. The orchestrator routes
    /// it to the next component in the chain.
    ///
    /// # Returns
    ///
    /// Returns a [`JsonRpcResponse`] that can be awaited to get the successor's
    /// response.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use scp::proxy::JsonRpcCxExt;
    /// use agent_client_protocol_schema::agent::PromptRequest;
    ///
    /// let prompt = PromptRequest { /* ... */ };
    /// let response = cx.send_request_to_successor(prompt).recv().await?;
    /// // response is the typed PromptResponse
    /// ```
    fn send_request_to_successor<Req: JsonRpcRequest>(
        &self,
        request: Req,
    ) -> scp::JsonRpcResponse<Req::Response>;

    /// Send a notification to the successor component.
    ///
    /// The notification is automatically wrapped in a `ToSuccessorNotification`
    /// and sent using the `_proxy/successor/send/notification` method. The
    /// orchestrator routes it to the next component in the chain.
    ///
    /// Notifications are fire-and-forget - no response is expected.
    ///
    /// # Errors
    ///
    /// Returns an error if the notification fails to send.
    fn send_notification_to_successor<Req: JsonRpcNotification>(
        &self,
        notification: Req,
    ) -> Result<(), acp::Error>;
}

impl JsonRpcCxExt for JsonRpcConnectionCx {
    fn send_request_to_successor<Req: JsonRpcRequest>(
        &self,
        request: Req,
    ) -> scp::JsonRpcResponse<Req::Response> {
        let wrapper = SuccessorRequest { request };
        self.send_request(wrapper)
    }

    fn send_notification_to_successor<Req: JsonRpcNotification>(
        &self,
        notification: Req,
    ) -> Result<(), acp::Error> {
        let wrapper = SuccessorNotification { notification };
        self.send_notification(wrapper)
    }
}
