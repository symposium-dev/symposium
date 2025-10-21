use crate::{
    jsonrpc::{
        ChainHandler, Handled, JsonRpcConnection, JsonRpcCx, JsonRpcHandler, JsonRpcRequestCx,
    },
    proxy::messages,
    util::json_cast,
};

/// Extension trait for [`JsonRpcConnection`] that adds S/ACP proxy capabilities.
///
/// This trait provides the [`on_receive_from_successor`](JsonRpcConnectionExt::on_receive_from_successor)
/// method for handling messages from downstream components (successors) in the proxy chain.
pub trait JsonRpcConnectionExt<H: JsonRpcHandler> {
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
    /// # async fn example() -> Result<(), jsonrpcmsg::Error> {
    /// JsonRpcConnection::new(tokio::io::stdin(), tokio::io::stdout())
    ///     .on_receive_from_successor(MyHandler)
    ///     .serve()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    fn on_receive_from_successor<H1>(
        self,
        handler: H1,
    ) -> JsonRpcConnection<ChainHandler<H, FromProxyHandler<H1>>>
    where
        H1: JsonRpcHandler;
}

impl<H: JsonRpcHandler> JsonRpcConnectionExt<H> for JsonRpcConnection<H> {
    fn on_receive_from_successor<H1>(
        self,
        handler: H1,
    ) -> JsonRpcConnection<ChainHandler<H, FromProxyHandler<H1>>>
    where
        H1: JsonRpcHandler,
    {
        self.on_receive(FromProxyHandler { handler })
    }
}

/// Handler wrapper that unwraps `_proxy/successor/receive/*` messages.
///
/// This type wraps a user-provided handler and intercepts messages from the successor
/// component. It automatically unwraps the protocol wrappers so the inner handler
/// receives normal ACP messages.
///
/// ## Protocol Handling
///
/// ### Requests (`_proxy/successor/receive/request`)
///
/// When a successor sends a request:
/// 1. Extract the inner ACP request from the wrapper
/// 2. Forward to the wrapped handler
/// 3. Wrap the handler's response in a `ToSuccessorResponse`
///
/// ### Notifications (`_proxy/successor/receive/notification`)
///
/// When a successor sends a notification:
/// 1. Extract the inner ACP notification from the wrapper
/// 2. Forward to the wrapped handler
///
/// ## Usage
///
/// You typically don't construct this directly. Instead, use
/// [`JsonRpcConnectionExt::on_receive_from_successor`].
pub struct FromProxyHandler<H>
where
    H: JsonRpcHandler,
{
    handler: H,
}

impl<H> JsonRpcHandler for FromProxyHandler<H>
where
    H: JsonRpcHandler,
{
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        if method != "_proxy/successor/receive/request" {
            return Ok(Handled::No(response));
        }

        // We have just received a request from the successor which looks like
        //
        // ```json
        // {
        //    "method": "_proxy/successor/receive/request",
        //    "id": $outer_id,
        //    "params": {
        //        "message": {
        //            "id": $inner_id,
        //            ...
        //        }
        //    }
        // }
        // ```
        //
        // What we want to do is to (1) remember ; (2) forward the inner message
        // to our handler. The handler will send us a response R and we want to
        //
        //
        //
        let messages::ReceiveFromSuccessorRequest {
            method: inner_method,
            params: inner_params,
        } = json_cast(params)?;

        // The user will send us a response that is intended for the proxy.
        // We repackage that into a `{message: ...}` struct that embeds
        // the response that will be sent to the proxy.
        let response = response.map(
            move |response: serde_json::Value| {
                serde_json::to_value(messages::FromSuccessorResponse::Result(response))
                    .map_err(|_| jsonrpcmsg::Error::internal_error())
            },
            move |error: jsonrpcmsg::Error| {
                serde_json::to_value(messages::FromSuccessorResponse::Error(error))
                    .map_err(|_| jsonrpcmsg::Error::internal_error())
            },
        );

        self.handler
            .handle_request(&inner_method, &inner_params, response)
            .await
    }

    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &JsonRpcCx,
    ) -> Result<Handled<()>, jsonrpcmsg::Error> {
        if method != "_proxy/successor/receive/notification" {
            return Ok(Handled::No(()));
        }

        let messages::FromSuccessorNotification {
            message:
                jsonrpcmsg::Request {
                    jsonrpc: _,
                    version: _,
                    method: inner_method,
                    params: inner_params,
                    id: None,
                },
        } = json_cast::<_, messages::FromSuccessorNotification>(params)?
        else {
            // We don't expect an `id` on a notification.
            return Err(jsonrpcmsg::Error::invalid_request());
        };

        self.handler
            .handle_notification(&inner_method, &inner_params, cx)
            .await
    }
}
