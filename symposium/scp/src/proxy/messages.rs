//! Message types for proxy communication with successor components.
//!
//! These types wrap JSON-RPC messages for the `_proxy/successor/*` protocol.

use serde::{Deserialize, Serialize};

use crate::jsonrpc::{JsonRpcNotification, JsonRpcRequest};

// ============================================================================
// Messages TO successor
// ============================================================================

/// A request being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a request downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorRequest {
    pub message: jsonrpcmsg::Request,
}

impl JsonRpcRequest for ToSuccessorRequest {
    type Response = FromSuccessorResponse;

    fn method(&self) -> &str {
        "_proxy/successor/send/request"
    }
}

/// A notification being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a notification downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorNotification {
    pub message: jsonrpcmsg::Request,
}

impl JsonRpcNotification for ToSuccessorNotification {
    fn method(&self) -> &str {
        "_proxy/successor/send/notification"
    }
}

// ============================================================================
// Messages FROM successor
// ============================================================================

/// A response received from the successor component.
///
/// Returned as the response to a `ToSuccessorRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorResponse {
    pub message: jsonrpcmsg::Response,
}

/// A request received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor wants to call back upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorRequest {
    pub message: jsonrpcmsg::Request,
}

impl JsonRpcRequest for FromSuccessorRequest {
    type Response = ToSuccessorResponse;

    fn method(&self) -> &str {
        "_proxy/successor/receive/request"
    }
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

// ============================================================================
// Response types for callback requests
// ============================================================================

/// Response to a request from the successor (FromSuccessorRequest).
///
/// When the successor calls back to the proxy/editor, the proxy responds with this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorResponse {
    pub message: jsonrpcmsg::Response,
}
