use std::error::Error;

use crate::{
    Handled, JsonRpcHandler, JsonRpcMessage, JsonRpcNotification, JsonRpcNotificationCx,
    JsonRpcOutgoingMessage, JsonRpcRequest, JsonRpcRequestCx, UntypedMessage,
};
use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

pub const METHOD_MCP_REQUEST: &str = "_mcp/request";

/// An MCP request sent via ACP. This could be an MCP-server-to-MCP-client request
/// (in which case it goes from the ACP client to the ACP agent,
/// note the reversal of roles) or an MCP-client-to-MCP-server request
/// (in which case it goes from the ACP agent to the ACP client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRequest<R> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: R,
}

impl<R: JsonRpcOutgoingMessage> JsonRpcMessage for McpRequest<R> {}

impl<R: JsonRpcOutgoingMessage> JsonRpcOutgoingMessage for McpRequest<R> {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(UntypedMessage::new(
            method,
            serde_json::to_value(McpRequest {
                method: params_msg.method,
                params: params_msg.params,
            })
            .map_err(acp::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        METHOD_MCP_REQUEST
    }
}

impl<R: JsonRpcRequest> JsonRpcRequest for McpRequest<R> {
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
pub struct McpNotification<R> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: R,
}

impl<R: JsonRpcOutgoingMessage> JsonRpcMessage for McpNotification<R> {}

impl<R: JsonRpcOutgoingMessage> JsonRpcOutgoingMessage for McpNotification<R> {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(UntypedMessage::new(
            method,
            serde_json::to_value(McpNotification {
                method: params_msg.method,
                params: params_msg.params,
            })
            .map_err(acp::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        METHOD_MCP_NOTIFICATION
    }
}

impl<R: JsonRpcOutgoingMessage> JsonRpcNotification for McpNotification<R> {}

/// Callbacks for "mcp-over-acp"
#[allow(async_fn_in_trait)]
pub trait AcpMcpCallbacks {
    async fn mcp_request(
        &mut self,
        request: McpRequest<serde_json::Value>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), acp::Error>;

    async fn mcp_notification(
        &mut self,
        request: McpNotification<serde_json::Value>,
        notification_cx: JsonRpcNotificationCx,
    ) -> Result<(), acp::Error>;
}

/// MCP-over-ACP messages
pub struct AcpMcpMessages<CB: AcpMcpCallbacks> {
    callbacks: CB,
}

impl<CB: AcpMcpCallbacks> AcpMcpMessages<CB> {
    pub fn callback(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: AcpMcpCallbacks> JsonRpcHandler for AcpMcpMessages<CB> {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::Handled<JsonRpcRequestCx<serde_json::Value>>, agent_client_protocol::Error>
    {
        if cx.method() != METHOD_MCP_REQUEST {
            return Ok(Handled::No(cx));
        }

        // TODO: implement MCP request handling
        Ok(Handled::No(cx))
    }

    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        _params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::Handled<JsonRpcNotificationCx>, agent_client_protocol::Error> {
        if cx.method() != METHOD_MCP_NOTIFICATION {
            return Ok(Handled::No(cx));
        }

        // TODO: implement MCP notification handling
        Ok(crate::Handled::No(cx))
    }
}

impl<TX, E> AcpMcpMessages<AcpMcpSendTo<TX, E>>
where
    TX: AsyncFnMut(AcpMcpMessage) -> Result<(), E>,
    E: Error,
{
    pub fn send_to(tx: TX) -> Self {
        Self::callback(AcpMcpSendTo { tx })
    }
}

pub enum AcpMcpMessage {
    Request(UntypedMessage, JsonRpcRequestCx<serde_json::Value>),
    Notification(UntypedMessage, JsonRpcNotificationCx),
}

pub struct AcpMcpSendTo<TX, E>
where
    TX: AsyncFnMut(AcpMcpMessage) -> Result<(), E>,
    E: Error,
{
    tx: TX,
}

impl<TX, E> AcpMcpCallbacks for AcpMcpSendTo<TX, E>
where
    TX: AsyncFnMut(AcpMcpMessage) -> Result<(), E>,
    E: Error,
{
    async fn mcp_request(
        &mut self,
        request: McpRequest<serde_json::Value>,
        request_cx: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), acp::Error> {
        let untyped = request.into_untyped_message()?;
        (self.tx)(AcpMcpMessage::Request(untyped, request_cx))
            .await
            .map_err(acp::Error::into_internal_error)
    }

    async fn mcp_notification(
        &mut self,
        request: McpNotification<serde_json::Value>,
        notification_cx: JsonRpcNotificationCx,
    ) -> Result<(), acp::Error> {
        let untyped = request.into_untyped_message()?;
        (self.tx)(AcpMcpMessage::Notification(untyped, notification_cx))
            .await
            .map_err(acp::Error::into_internal_error)
    }
}
