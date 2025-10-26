//! Message types for proxy communication with successor components.
//!
//! These types wrap JSON-RPC messages for the `_proxy/successor/*` protocol.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest};

// ============================================================================
// Requests and notifications send TO successor (and the response we receieve)
// ============================================================================

const TO_SUCCESSOR_REQUEST_METHOD: &str = "_proxy/successor/send/request";

/// A request being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a request downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorRequest<Req: JsonRpcRequest> {
    /// The message to be sent to the successor component.
    #[serde(flatten)]
    pub request: Req,
}

impl<Req: JsonRpcRequest> JsonRpcMessage for ToSuccessorRequest<Req> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        crate::UntypedMessage::new(
            TO_SUCCESSOR_REQUEST_METHOD,
            self.request.into_untyped_message()?,
        )
    }

    fn method(&self) -> &str {
        TO_SUCCESSOR_REQUEST_METHOD
    }
}

impl<Req: JsonRpcRequest> JsonRpcRequest for ToSuccessorRequest<Req> {
    type Response = Req::Response;
}

const TO_SUCCESSOR_NOTIFICATION_METHOD: &str = "_proxy/successor/send/notification";

/// A notification being sent to the successor component.
///
/// Used in `_proxy/successor/send` when the proxy wants to forward a notification downstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToSuccessorNotification<Req: JsonRpcNotification> {
    /// The message to be sent to the successor component.
    #[serde(flatten)]
    pub notification: Req,
}

impl<Req: JsonRpcNotification> JsonRpcMessage for ToSuccessorNotification<Req> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        crate::UntypedMessage::new(
            TO_SUCCESSOR_NOTIFICATION_METHOD,
            self.notification.into_untyped_message()?,
        )
    }

    fn method(&self) -> &str {
        TO_SUCCESSOR_NOTIFICATION_METHOD
    }
}

impl<Req: JsonRpcNotification> JsonRpcNotification for ToSuccessorNotification<Req> {}

// ============================================================================
// Messages FROM successor
// ============================================================================

const FROM_SUCCESSOR_REQUEST_METHOD: &str = "_proxy/successor/receive/request";

/// A request received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor wants to call back upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FromSuccessorRequest<Req: JsonRpcRequest> {
    /// The message received from the successor component.
    #[serde(flatten)]
    pub request: Req,
}

impl<R: JsonRpcRequest> JsonRpcMessage for FromSuccessorRequest<R> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        crate::UntypedMessage::new(
            FROM_SUCCESSOR_REQUEST_METHOD,
            self.request.into_untyped_message()?,
        )
    }

    fn method(&self) -> &str {
        FROM_SUCCESSOR_REQUEST_METHOD
    }
}

impl<R: JsonRpcRequest> JsonRpcRequest for FromSuccessorRequest<R> {
    type Response = R::Response;
}

const FROM_SUCCESSOR_NOTIFICATION_METHOD: &str = "_proxy/successor/receive/notification";

/// A notification received from the successor component.
///
/// Delivered via `_proxy/successor/receive` when the successor sends a notification upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromSuccessorNotification<N: JsonRpcNotification> {
    /// The message received from the successor component.
    #[serde(flatten)]
    pub notification: N,
}

impl<N: JsonRpcNotification> JsonRpcMessage for FromSuccessorNotification<N> {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        crate::UntypedMessage::new(
            FROM_SUCCESSOR_NOTIFICATION_METHOD,
            self.notification.into_untyped_message()?,
        )
    }

    fn method(&self) -> &str {
        FROM_SUCCESSOR_NOTIFICATION_METHOD
    }
}

impl<N: JsonRpcNotification> JsonRpcNotification for FromSuccessorNotification<N> {}
