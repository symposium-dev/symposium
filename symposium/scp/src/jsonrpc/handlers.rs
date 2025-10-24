use crate::JsonRpcNotificationCx;
use crate::jsonrpc::{Handled, JsonRpcHandler};
use std::error::Error;
use std::ops::AsyncFnMut;

use super::JsonRpcRequestCx;

#[derive(Default)]
pub struct NullHandler {}

impl JsonRpcHandler for NullHandler {}

pub struct ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    handler1: H1,
    handler2: H2,
}

impl<H1, H2> ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    pub fn new(handler1: H1, handler2: H2) -> Self {
        Self { handler1, handler2 }
    }
}

impl<H1, H2> JsonRpcHandler for ChainHandler<H1, H2>
where
    H1: JsonRpcHandler,
    H2: JsonRpcHandler,
{
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        match self.handler1.handle_request(cx, params).await? {
            Handled::Yes => Ok(Handled::Yes),
            Handled::No(cx) => self.handler2.handle_request(cx, params).await,
        }
    }

    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcNotificationCx>, jsonrpcmsg::Error> {
        match self.handler1.handle_notification(cx, params).await? {
            Handled::Yes => Ok(Handled::Yes),
            Handled::No(cx) => self.handler2.handle_notification(cx, params).await,
        }
    }
}

/// Generic JSON-RPC handler that forwards all incoming messages to a callback function.
///
/// This is useful for bridging JSON-RPC messages to an mpsc channel or other routing mechanism,
/// allowing centralized message handling in an event loop.
pub struct GenericHandler<TX, E>
where
    TX: AsyncFnMut(
        String,
        Option<jsonrpcmsg::Params>,
        JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), E>,
    E: Error,
{
    tx: TX,
}

impl<TX, E> GenericHandler<TX, E>
where
    TX: AsyncFnMut(
        String,
        Option<jsonrpcmsg::Params>,
        JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), E>,
    E: Error,
{
    /// Create a handler that forwards all requests to the given callback.
    ///
    /// The callback receives:
    /// - `method`: The JSON-RPC method name
    /// - `params`: The JSON-RPC parameters (if any)
    /// - `response_cx`: Context for sending the response
    ///
    /// Example usage:
    /// ```ignore
    /// connection
    ///     .on_receive(GenericHandler::send_to(|method, params, response_cx| async move {
    ///         // Forward to mpsc channel
    ///         tx.send((method, params, response_cx)).await?;
    ///         Ok(())
    ///     }))
    ///     .serve()
    ///     .await
    /// ```
    pub fn send_to(tx: TX) -> Self {
        Self { tx }
    }
}

impl<TX, E> JsonRpcHandler for GenericHandler<TX, E>
where
    TX: AsyncFnMut(
        String,
        Option<jsonrpcmsg::Params>,
        JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), E>,
    E: Error,
{
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        (self.tx)(cx.method().to_string(), params.clone(), cx)
            .await
            .map_err(|e| {
                jsonrpcmsg::Error::with_data(
                    -32603, // Internal error
                    "Internal error".to_string(),
                    serde_json::json!({"error": e.to_string()}),
                )
            })?;

        // Always claim the message (GenericHandler handles everything)
        Ok(Handled::Yes)
    }

    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcNotificationCx>, jsonrpcmsg::Error> {
        // Generic handler only handles requests, not notifications
        // (notifications don't need responses, so they don't fit the bridge use case)
        Ok(Handled::No(cx))
    }
}
