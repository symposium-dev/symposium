//! Message types for proxy communication with successor components.
//!
//! These types wrap JSON-RPC messages for the `_proxy/successor/*` protocol.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage, JsonRpcRequest};

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
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(ToSuccessorRequest {
                method: params_msg.method,
                params: params_msg.params,
            }).map_err(acp::Error::into_internal_error)?,
        ))
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
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(ToSuccessorRequest {
                method: params_msg.method,
                params: params_msg.params,
            }).map_err(acp::Error::into_internal_error)?,
        ))
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
pub struct FromSuccessorRequest<R> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: R,
}

impl<R: JsonRpcRequest> JsonRpcMessage for FromSuccessorRequest<R> {}

impl<R: JsonRpcRequest> JsonRpcOutgoingMessage for FromSuccessorRequest<R> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(ToSuccessorRequest {
                method: params_msg.method,
                params: params_msg.params,
            }).map_err(acp::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "_proxy/successor/receive/request"
    }
}

impl<R: JsonRpcRequest> JsonRpcRequest for FromSuccessorRequest<R> {
    type Response = R::Response;
}

/// A notification received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor sends a notification upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorNotification<N> {
    /// Name of the method to be invoked
    pub method: String,

    /// Parameters for the method invocation
    pub params: N,
}

impl<N: JsonRpcNotification> JsonRpcMessage for FromSuccessorNotification<N> {}

impl<N: JsonRpcNotification> JsonRpcOutgoingMessage for FromSuccessorNotification<N> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        let params_msg = self.params.into_untyped_message()?;
                Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(ToSuccessorRequest {
                method: params_msg.method,
                params: params_msg.params,
            }).map_err(acp::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "_proxy/successor/receive/notification"
    }
}

impl<N: JsonRpcNotification> JsonRpcNotification for FromSuccessorNotification<N> {}
