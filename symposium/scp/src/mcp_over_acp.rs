use std::error::Error;

use crate::{
    Handled, JsonRpcHandler, JsonRpcMessage, JsonRpcNotification, JsonRpcNotificationCx,
    JsonRpcRequest, JsonRpcRequestCx, JsonRpcResponsePayload, UntypedMessage, util::json_cast,
};
use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

pub const METHOD_MCP_CONNECT_REQUEST: &str = "_mcp/connect";

/// Creates a new MCP connection. This is equivalent to "running the command".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConnectRequest {
    pub acp_url: String,
}

impl JsonRpcMessage for McpConnectRequest {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        UntypedMessage::new(METHOD_MCP_CONNECT_REQUEST, self)
    }

    fn method(&self) -> &str {
        METHOD_MCP_CONNECT_REQUEST
    }
}

impl JsonRpcRequest for McpConnectRequest {
    type Response = McpConnectResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConnectResponse {
    pub connection_id: String,
}

impl JsonRpcResponsePayload for McpConnectResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(acp::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        serde_json::from_value(value).map_err(|_| acp::Error::invalid_params())
    }
}

pub const METHOD_MCP_DISCONNECT_NOTIFICATION: &str = "_mcp/disconnect";

/// Disconnects the MCP connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpDisconnectNotification {
    /// The id of the connection to disconnect.
    pub connection_id: String,
}

impl JsonRpcMessage for McpDisconnectNotification {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        UntypedMessage::new(METHOD_MCP_DISCONNECT_NOTIFICATION, self)
    }

    fn method(&self) -> &str {
        METHOD_MCP_DISCONNECT_NOTIFICATION
    }
}

impl JsonRpcNotification for McpDisconnectNotification {}

pub const METHOD_MCP_REQUEST: &str = "_mcp/request";

/// An MCP request sent via ACP. This could be an MCP-server-to-MCP-client request
/// (in which case it goes from the ACP client to the ACP agent,
/// note the reversal of roles) or an MCP-client-to-MCP-server request
/// (in which case it goes from the ACP agent to the ACP client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOverAcpRequest<R> {
    /// id given in response to `_mcp/connect` request.
    pub connection_id: String,

    /// Request to be sent to the MCP server or client.
    #[serde(flatten)]
    pub message: R,
}

impl<R: JsonRpcRequest> JsonRpcMessage for McpOverAcpRequest<R> {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        let message = self.message.into_untyped_message()?;
        UntypedMessage::new(METHOD_MCP_REQUEST, message)
    }

    fn method(&self) -> &str {
        METHOD_MCP_REQUEST
    }
}

impl<R: JsonRpcRequest> JsonRpcRequest for McpOverAcpRequest<R> {
    type Response = R::Response;
}

pub const METHOD_MCP_NOTIFICATION: &str = "_mcp/notification";

/// An MCP notification sent via ACP, either from the MCP client (the ACP agent)
/// or the MCP server (the ACP client).
///
/// Delivered via `_mcp/notification` when the MCP client (the ACP agent)
/// sends a notification to the MCP server (the ACP client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOverAcpNotification<R> {
    /// id given in response to `_mcp/connect` request.
    pub connection_id: String,

    /// Notification to be sent to the MCP server or client.
    #[serde(flatten)]
    pub notification: R,
}

impl<R: JsonRpcMessage> JsonRpcMessage for McpOverAcpNotification<R> {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        let params = self.notification.into_untyped_message()?;
        UntypedMessage::new(METHOD_MCP_NOTIFICATION, params)
    }

    fn method(&self) -> &str {
        METHOD_MCP_NOTIFICATION
    }
}

impl<R: JsonRpcMessage> JsonRpcNotification for McpOverAcpNotification<R> {}

/// Callbacks for "mcp-over-acp"
#[allow(async_fn_in_trait)]
pub trait McpOverAcpCallbacks {
    async fn mcp_request(
        &mut self,
        request: McpOverAcpRequest<UntypedMessage>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), acp::Error>;

    async fn mcp_notification(
        &mut self,
        request: McpOverAcpNotification<UntypedMessage>,
        notification_cx: JsonRpcNotificationCx,
    ) -> Result<(), acp::Error>;
}

/// MCP-over-ACP messages
pub struct McpOverAcpMessages<CB: McpOverAcpCallbacks> {
    callbacks: CB,
}

impl<CB: McpOverAcpCallbacks> McpOverAcpMessages<CB> {
    pub fn callback(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: McpOverAcpCallbacks> JsonRpcHandler for McpOverAcpMessages<CB> {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error>
    {
        if cx.method() != METHOD_MCP_REQUEST {
            return Ok(Handled::No(cx));
        }

        let request: McpOverAcpRequest<UntypedMessage> = json_cast(params)?;
        self.callbacks.mcp_request(request, cx).await?;
        Ok(Handled::Yes)
    }

    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::Handled<JsonRpcNotificationCx>, agent_client_protocol::Error> {
        if cx.method() != METHOD_MCP_NOTIFICATION {
            return Ok(Handled::No(cx));
        }

        let request: McpOverAcpNotification<UntypedMessage> = json_cast(params)?;
        self.callbacks.mcp_notification(request, cx).await?;
        Ok(Handled::Yes)
    }
}

impl<TX, E> McpOverAcpMessages<AcpMcpSendTo<TX, E>>
where
    TX: AsyncFnMut(McpOverAcpMessage) -> Result<(), E>,
    E: Error,
{
    pub fn send_to(tx: TX) -> Self {
        Self::callback(AcpMcpSendTo { tx })
    }
}

/// An MCP message sent over ACP.
pub enum McpOverAcpMessage {
    /// An MCP request requiring a reply.
    Request(
        McpOverAcpRequest<UntypedMessage>,
        JsonRpcRequestCx<serde_json::Value>,
    ),
    /// An MCP notification.
    Notification(
        McpOverAcpNotification<UntypedMessage>,
        JsonRpcNotificationCx,
    ),
}

/// MCP-over-ACP callbacks to send [`McpOverAcpMessage`] to a channel.
pub struct AcpMcpSendTo<TX, E>
where
    TX: AsyncFnMut(McpOverAcpMessage) -> Result<(), E>,
    E: Error,
{
    tx: TX,
}

impl<TX, E> McpOverAcpCallbacks for AcpMcpSendTo<TX, E>
where
    TX: AsyncFnMut(McpOverAcpMessage) -> Result<(), E>,
    E: Error,
{
    async fn mcp_request(
        &mut self,
        request: McpOverAcpRequest<UntypedMessage>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), acp::Error> {
        (self.tx)(McpOverAcpMessage::Request(request, request_cx))
            .await
            .map_err(acp::Error::into_internal_error)
    }

    async fn mcp_notification(
        &mut self,
        notification: McpOverAcpNotification<UntypedMessage>,
        notification_cx: JsonRpcNotificationCx,
    ) -> Result<(), acp::Error> {
        (self.tx)(McpOverAcpMessage::Notification(
            notification,
            notification_cx,
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }
}
