//! Proxy support for the Symposium Component Protocol (S/ACP).
//!
//! This module provides utilities for building proxy components that sit between
//! editors and agents in an S/ACP chain. Proxies can intercept, transform, and
//! forward messages in both directions.
//!
//! # Core Concepts
//!
//! ## Message Flow
//!
//! In an S/ACP chain, messages flow through proxies:
//!
//! ```text
//! Editor → Proxy → Agent
//!        ↓      ↓
//!    (upstream) (downstream/successor)
//! ```
//!
//! - **Upstream**: Messages from/to the editor
//! - **Downstream/Successor**: Messages from/to the next component (another proxy or agent)
//!
//! ## Handler Abstraction
//!
//! The [`FromProxyHandler`] wrapper allows proxy authors to write handlers that
//! process normal ACP messages without dealing with the `_proxy/successor/receive/*`
//! protocol wrappers. The handler automatically unwraps incoming messages from
//! successors and rewraps responses.
//!
//! # Example
//!
//! ```rust,no_run
//! use scp::proxy::JsonRpcConnectionExt;
//! use scp::jsonrpc::{JsonRpcConnection, JsonRpcHandler};
//!
//! // Your handler processes normal ACP messages
//! struct MyProxyHandler;
//! impl JsonRpcHandler for MyProxyHandler {
//!     // Handle requests and notifications like any ACP component
//! }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! JsonRpcConnection::new(tokio::io::stdin(), tokio::io::stdout())
//!     .on_receive_from_successor(MyProxyHandler)
//!     .serve()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::{
    jsonrpc::{
        ChainHandler, Handled, JsonRpcConnection, JsonRpcCx, JsonRpcHandler, JsonRpcRequestCx,
    },
    util::json_cast,
};

mod messages;

pub use messages::*;

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
    /// ```rust,no_run
    /// # use scp::proxy::JsonRpcConnectionExt;
    /// # use scp::jsonrpc::{JsonRpcConnection, JsonRpcHandler};
    /// # struct MyHandler;
    /// # impl JsonRpcHandler for MyHandler {}
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
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
        let messages::FromSuccessorRequest {
            message:
                jsonrpcmsg::Request {
                    jsonrpc: inner_jsonrpc,
                    version: inner_version,
                    method: inner_method,
                    params: inner_params,
                    id: inner_id,
                },
        } = json_cast::<_, messages::FromSuccessorRequest>(params)?;

        // The user will send us a response that is intended for the proxy.
        // We repackage that into a `{message: ...}` struct that embeds
        // the response that will be sent to the proxy.
        let response = response.map(
            {
                let inner_jsonrpc = inner_jsonrpc.clone();
                let inner_version = inner_version.clone();
                let inner_id = inner_id.clone();
                move |response: serde_json::Value| {
                    serde_json::to_value(messages::ToSuccessorResponse {
                        message: jsonrpcmsg::Response {
                            jsonrpc: inner_jsonrpc.clone(),
                            version: inner_version.clone(),
                            result: Some(response),
                            error: None,
                            id: inner_id.clone(),
                        },
                    })
                    .map_err(|_| jsonrpcmsg::Error::internal_error())
                }
            },
            move |error: jsonrpcmsg::Error| {
                serde_json::to_value(messages::ToSuccessorResponse {
                    message: jsonrpcmsg::Response {
                        jsonrpc: inner_jsonrpc.clone(),
                        version: inner_version.clone(),
                        result: None,
                        error: Some(error),
                        id: inner_id.clone(),
                    },
                })
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

/// Extension trait for [`JsonRpcCx`] that adds methods for sending to successor.
///
/// This trait provides convenient methods for proxies to forward messages downstream
/// to their successor component (next proxy or agent). Messages are automatically
/// wrapped in the `_proxy/successor/send/*` protocol format.
///
/// # Example
///
/// ```rust,no_run
/// # use scp::proxy::JsonRpcCxExt;
/// # use scp::jsonrpc::JsonRpcCx;
/// # async fn example(cx: &JsonRpcCx) -> Result<(), jsonrpcmsg::Error> {
/// // Forward a request to the successor
/// let request = jsonrpcmsg::Request::new(
///     "some_method".to_string(),
///     None,
///     Some(jsonrpcmsg::Id::Number(1)),
/// );
/// let response = cx.send_request_to_successor(request).await?;
///
/// // Forward a notification to the successor
/// let notification = jsonrpcmsg::Request::notification(
///     "some_notification".to_string(),
///     None,
/// );
/// cx.send_notification_to_successor(notification)?;
/// # Ok(())
/// # }
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
    /// response. The response will be a `FromSuccessorResponse` containing the
    /// inner `jsonrpcmsg::Response`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use scp::proxy::JsonRpcCxExt;
    /// # use scp::jsonrpc::{JsonRpcCx, JsonRpcResponse};
    /// # async fn example(cx: &JsonRpcCx) -> Result<(), jsonrpcmsg::Error> {
    /// let request = jsonrpcmsg::Request::new(
    ///     "some_method".to_string(),
    ///     None,
    ///     Some(jsonrpcmsg::Id::Number(1)),
    /// );
    /// let response = cx.send_request_to_successor(request).recv().await?;
    /// // response.message contains the inner jsonrpcmsg::Response
    /// # Ok(())
    /// # }
    /// ```
    fn send_request_to_successor(
        &self,
        request: jsonrpcmsg::Request,
    ) -> crate::jsonrpc::JsonRpcResponse<FromSuccessorResponse>;

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
    fn send_notification_to_successor(
        &self,
        notification: jsonrpcmsg::Request,
    ) -> Result<(), jsonrpcmsg::Error>;
}

impl JsonRpcCxExt for JsonRpcCx {
    fn send_request_to_successor(
        &self,
        request: jsonrpcmsg::Request,
    ) -> crate::jsonrpc::JsonRpcResponse<FromSuccessorResponse> {
        let wrapper = ToSuccessorRequest { message: request };
        self.send_request(wrapper)
    }

    fn send_notification_to_successor(
        &self,
        notification: jsonrpcmsg::Request,
    ) -> Result<(), jsonrpcmsg::Error> {
        let wrapper = ToSuccessorNotification {
            message: notification,
        };
        self.send_notification(wrapper)
    }
}
