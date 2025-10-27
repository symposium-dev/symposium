use crate::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponsePayload, UntypedMessage,
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

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != METHOD_MCP_CONNECT_REQUEST {
            return None;
        }
        Some(crate::util::json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    fn parse_request(_method: &str, _params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        // This is a notification, not a request
        None
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        if method != METHOD_MCP_DISCONNECT_NOTIFICATION {
            return None;
        }
        Some(crate::util::json_cast(params))
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
    pub request: R,
}

impl<R: JsonRpcRequest> JsonRpcMessage for McpOverAcpRequest<R> {
    fn into_untyped_message(self) -> Result<UntypedMessage, acp::Error> {
        let message = self.request.into_untyped_message()?;
        UntypedMessage::new(METHOD_MCP_REQUEST, message)
    }

    fn method(&self) -> &str {
        METHOD_MCP_REQUEST
    }

    fn parse_request(_method: &str, _params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        // Generic wrapper type - cannot be parsed without knowing concrete inner type
        None
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // Generic wrapper type - cannot be parsed without knowing concrete inner type
        None
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

    fn parse_request(_method: &str, _params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        // Generic wrapper type - cannot be parsed without knowing concrete inner type
        None
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // Generic wrapper type - cannot be parsed without knowing concrete inner type
        None
    }
}

impl<R: JsonRpcMessage> JsonRpcNotification for McpOverAcpNotification<R> {}
