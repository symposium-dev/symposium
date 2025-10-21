//! Message types for proxy communication with successor components.
//!
//! These types wrap JSON-RPC messages for the `_proxy/successor/*` protocol.

use serde::{Deserialize, Serialize};

use crate::jsonrpc::{JsonRpcNotification, JsonRpcRequest};

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

impl<Req: JsonRpcRequest> JsonRpcRequest for ToSuccessorRequest<Req> {
    type Response = ToSuccessorResponse<Req::Response>;

    fn method(&self) -> &str {
        "_proxy/successor/send/request"
    }
}

/// A response received from a [`ToSuccessorRequest`].
///
/// Returned as the response to a `ToSuccessorRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToSuccessorResponse<Response> {
    /// Result of the method invocation (on success)
    Result(Response),

    /// Error object (on failure)
    Error(jsonrpcmsg::Error),
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

impl<Req: JsonRpcNotification> JsonRpcNotification for ToSuccessorNotification<Req> {
    fn method(&self) -> &str {
        "_proxy/successor/send/notification"
    }
}

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

impl JsonRpcRequest for ReceiveFromSuccessorRequest {
    type Response = FromSuccessorResponse;

    fn method(&self) -> &str {
        "_proxy/successor/receive/request"
    }
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

/// A notification received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor sends a notification upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorNotification {
    pub message: jsonrpcmsg::Request,
}

impl JsonRpcNotification for FromSuccessorNotification {
    fn method(&self) -> &str {
        "_proxy/successor/receive/notification"
    }
}
