use agent_client_protocol::{
    AuthenticateRequest, AuthenticateResponse, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SetSessionModeRequest, SetSessionModeResponse,
};
use serde::Serialize;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponsePayload};
use crate::util::json_cast;

// ============================================================================
// InitializeRequest
// ============================================================================

impl JsonRpcMessage for InitializeRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "initialize"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "initialize" {
            return None;
        }

        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for InitializeRequest {
    type Response = InitializeResponse;
}

impl JsonRpcResponsePayload for InitializeResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// AuthenticateRequest
// ============================================================================

impl JsonRpcMessage for AuthenticateRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "authenticate"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "authenticate" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for AuthenticateRequest {
    type Response = AuthenticateResponse;
}

impl JsonRpcResponsePayload for AuthenticateResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// LoadSessionRequest
// ============================================================================

impl JsonRpcMessage for LoadSessionRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/load"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/load" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for LoadSessionRequest {
    type Response = LoadSessionResponse;
}

impl JsonRpcResponsePayload for LoadSessionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// NewSessionRequest
// ============================================================================

impl JsonRpcMessage for NewSessionRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/new"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/new" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for NewSessionRequest {
    type Response = NewSessionResponse;
}

impl JsonRpcResponsePayload for NewSessionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// PromptRequest
// ============================================================================

impl JsonRpcMessage for PromptRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/prompt"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/prompt" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for PromptRequest {
    type Response = PromptResponse;
}

impl JsonRpcResponsePayload for PromptResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// SetSessionModeRequest
// ============================================================================

impl JsonRpcMessage for SetSessionModeRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/set_mode"
    }

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/set_mode" {
            return None;
        }
        Some(json_cast(params))
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a request, not a notification
        None
    }
}

impl JsonRpcRequest for SetSessionModeRequest {
    type Response = SetSessionModeResponse;
}

impl JsonRpcResponsePayload for SetSessionModeResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
        serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)
    }

    fn from_value(
        _method: &str,
        value: serde_json::Value,
    ) -> Result<Self, agent_client_protocol::Error> {
        json_cast(&value)
    }
}
