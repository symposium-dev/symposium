use crate::{
    jsonrpc::{JsonRpcConnectionCx, JsonRpcNotification, JsonRpcRequest},
    proxy::{ToSuccessorNotification, ToSuccessorRequest},
};
use agent_client_protocol as acp;

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
    /// response. The response will be a `FromSuccessorResponse` containing the
    /// inner `jsonrpcmsg::Response`.
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
    ) -> crate::jsonrpc::JsonRpcResponse<Req::Response>;

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
    ) -> crate::jsonrpc::JsonRpcResponse<Req::Response> {
        let wrapper = ToSuccessorRequest {
            method: request.method().to_string(),
            params: request,
        };

        self.send_request(wrapper)
    }

    fn send_notification_to_successor<Req: JsonRpcNotification>(
        &self,
        notification: Req,
    ) -> Result<(), acp::Error> {
        let wrapper = ToSuccessorNotification {
            method: notification.method().to_string(),
            params: notification,
        };
        self.send_notification(wrapper)
    }
}
