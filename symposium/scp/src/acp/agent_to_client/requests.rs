use agent_client_protocol::{
    self as acp, CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionRequest, RequestPermissionResponse,
    TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use serde::Serialize;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponsePayload};
use crate::util::json_cast;

// Agent -> Client requests
// These are messages that agents send to clients/editors

// ============================================================================
// RequestPermissionRequest
// ============================================================================

impl JsonRpcMessage for RequestPermissionRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/request_permission"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "session/request_permission" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for RequestPermissionRequest {
    type Response = RequestPermissionResponse;
}

impl JsonRpcResponsePayload for RequestPermissionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// WriteTextFileRequest
// ============================================================================

impl JsonRpcMessage for WriteTextFileRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "fs/write_text_file"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "fs/write_text_file" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for WriteTextFileRequest {
    type Response = WriteTextFileResponse;
}

impl JsonRpcResponsePayload for WriteTextFileResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// ReadTextFileRequest
// ============================================================================

impl JsonRpcMessage for ReadTextFileRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "fs/read_text_file"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "fs/read_text_file" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for ReadTextFileRequest {
    type Response = ReadTextFileResponse;
}

impl JsonRpcResponsePayload for ReadTextFileResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// CreateTerminalRequest
// ============================================================================

impl JsonRpcMessage for CreateTerminalRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "terminal/create"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "terminal/create" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for CreateTerminalRequest {
    type Response = CreateTerminalResponse;
}

impl JsonRpcResponsePayload for CreateTerminalResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// TerminalOutputRequest
// ============================================================================

impl JsonRpcMessage for TerminalOutputRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "terminal/output"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "terminal/output" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for TerminalOutputRequest {
    type Response = TerminalOutputResponse;
}

impl JsonRpcResponsePayload for TerminalOutputResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// ReleaseTerminalRequest
// ============================================================================

impl JsonRpcMessage for ReleaseTerminalRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "terminal/release"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "terminal/release" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for ReleaseTerminalRequest {
    type Response = ReleaseTerminalResponse;
}

impl JsonRpcResponsePayload for ReleaseTerminalResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// WaitForTerminalExitRequest
// ============================================================================

impl JsonRpcMessage for WaitForTerminalExitRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "terminal/wait_for_exit"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "terminal/wait_for_exit" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for WaitForTerminalExitRequest {
    type Response = WaitForTerminalExitResponse;
}

impl JsonRpcResponsePayload for WaitForTerminalExitResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// KillTerminalCommandRequest
// ============================================================================

impl JsonRpcMessage for KillTerminalCommandRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "terminal/kill"
    }

    fn parse_request(method: &str, params: &impl Serialize) -> Option<Result<Self, acp::Error>> {
        if method != "terminal/kill" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, acp::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for KillTerminalCommandRequest {
    type Response = KillTerminalCommandResponse;
}

impl JsonRpcResponsePayload for KillTerminalCommandResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, acp::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, acp::Error> {
        json_cast(&value)
    }
}
