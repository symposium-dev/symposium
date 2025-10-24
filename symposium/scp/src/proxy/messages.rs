//! Message types for proxy communication with successor components.
//!
//! These types wrap JSON-RPC messages for the `_proxy/successor/*` protocol.

use serde::{Deserialize, Serialize};

use crate::jsonrpc::{
    JsonRpcIncomingMessage, JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage,
    JsonRpcRequest,
};
use crate::util::json_cast;

// ============================================================================
// Requests and notifications send TO successor (and the response we receieve)
// ============================================================================

/// A request being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a request downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorRequest<Req> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: Req,
}

impl<Req: JsonRpcMessage> JsonRpcMessage for ToSuccessorRequest<Req> {}

impl<Req: JsonRpcOutgoingMessage> JsonRpcOutgoingMessage for ToSuccessorRequest<Req> {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(ToSuccessorRequest {
            method: self.method,
            params: self.params.params()?,
        })
    }

    fn method(&self) -> &str {
        "_proxy/successor/send/request"
    }
}

impl<Req: JsonRpcRequest> JsonRpcRequest for ToSuccessorRequest<Req> {
    type Response = Req::Response;
}

/// A notification being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a notification downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorNotification<Req> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: Req,
}

impl<Req: JsonRpcMessage> JsonRpcMessage for ToSuccessorNotification<Req> {}

impl<Req: JsonRpcOutgoingMessage> JsonRpcOutgoingMessage for ToSuccessorNotification<Req> {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(ToSuccessorNotification {
            method: self.method,
            params: self.params.params()?,
        })
    }

    fn method(&self) -> &str {
        "_proxy/successor/send/notification"
    }
}

impl<Req: JsonRpcNotification> JsonRpcNotification for ToSuccessorNotification<Req> {}

// ============================================================================
// Messages FROM successor
// ============================================================================

/// A request received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor wants to call back upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveFromSuccessorRequest {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<jsonrpcmsg::Params>,
}

impl JsonRpcMessage for ReceiveFromSuccessorRequest {}

impl JsonRpcOutgoingMessage for ReceiveFromSuccessorRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "_proxy/successor/receive/request"
    }
}

impl JsonRpcRequest for ReceiveFromSuccessorRequest {
    type Response = FromSuccessorResponse;
}

/// Response sent when we receive a [`FromSuccessorRequest`]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FromSuccessorResponse {
    /// Result of the method invocation (on success)
    Result(serde_json::Value),

    /// Error object (on failure)
    Error(jsonrpcmsg::Error),
}

impl JsonRpcMessage for FromSuccessorResponse {}

impl JsonRpcIncomingMessage for FromSuccessorResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

/// A notification received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor sends a notification upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorNotification {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<jsonrpcmsg::Params>,
}

impl JsonRpcMessage for FromSuccessorNotification {}

impl JsonRpcOutgoingMessage for FromSuccessorNotification {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "_proxy/successor/receive/notification"
    }
}

impl JsonRpcNotification for FromSuccessorNotification {}
