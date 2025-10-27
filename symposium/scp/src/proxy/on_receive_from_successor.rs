use agent_client_protocol as acp;
use futures::{AsyncRead, AsyncWrite};

use crate::{
    FromSuccessorNotification, FromSuccessorRequest, JsonRpcConnection, JsonRpcHandler,
    JsonRpcNotification, JsonRpcNotificationCx, JsonRpcRequest, JsonRpcRequestCx,
};

/// Extension trait for [`JsonRpcConnection`] that adds S/ACP proxy capabilities.
///
/// This trait provides methods for handling messages from downstream components (successors)
/// in the proxy chain.
pub trait JsonRpcConnectionExt<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> {
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
    ) -> JsonRpcConnection<OB, IB, impl JsonRpcHandler>
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
    ) -> JsonRpcConnection<OB, IB, impl JsonRpcHandler>
    where
        N: JsonRpcNotification,
        F: AsyncFnMut(N, JsonRpcNotificationCx) -> Result<(), acp::Error>;
}

impl<OB: AsyncWrite, IB: AsyncRead, H: JsonRpcHandler> JsonRpcConnectionExt<OB, IB, H>
    for JsonRpcConnection<OB, IB, H>
{
    fn on_receive_request_from_successor<R, F>(
        self,
        mut op: F,
    ) -> JsonRpcConnection<OB, IB, impl JsonRpcHandler>
    where
        R: JsonRpcRequest,
        F: AsyncFnMut(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
    {
        self.on_receive_request(async move |request: FromSuccessorRequest<R>, request_cx| {
            op(request.request, request_cx).await
        })
    }

    fn on_receive_notification_from_successor<N, F>(
        self,
        mut op: F,
    ) -> JsonRpcConnection<OB, IB, impl JsonRpcHandler>
    where
        N: JsonRpcNotification,
        F: AsyncFnMut(N, JsonRpcNotificationCx) -> Result<(), acp::Error>,
    {
        self.on_receive_notification(
            async move |notification: FromSuccessorNotification<N>, cx| {
                op(notification.notification, cx).await
            },
        )
    }
}
