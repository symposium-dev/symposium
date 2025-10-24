use agent_client_protocol::{
    AuthenticateRequest, AuthenticateResponse, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SetSessionModeRequest, SetSessionModeResponse,
};

use crate::jsonrpc::{
    JsonRpcIncomingMessage, JsonRpcMessage, JsonRpcOutgoingMessage, JsonRpcRequest,
};
use crate::util::json_cast;

// ============================================================================
// InitializeRequest
// ============================================================================

impl JsonRpcMessage for InitializeRequest {}

impl JsonRpcOutgoingMessage for InitializeRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "initialize"
    }
}

impl JsonRpcRequest for InitializeRequest {
    type Response = InitializeResponse;
}

impl JsonRpcMessage for InitializeResponse {}

impl JsonRpcIncomingMessage for InitializeResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// AuthenticateRequest
// ============================================================================

impl JsonRpcMessage for AuthenticateRequest {}

impl JsonRpcOutgoingMessage for AuthenticateRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "authenticate"
    }
}

impl JsonRpcRequest for AuthenticateRequest {
    type Response = AuthenticateResponse;
}

impl JsonRpcMessage for AuthenticateResponse {}

impl JsonRpcIncomingMessage for AuthenticateResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// LoadSessionRequest
// ============================================================================

impl JsonRpcMessage for LoadSessionRequest {}

impl JsonRpcOutgoingMessage for LoadSessionRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/load"
    }
}

impl JsonRpcRequest for LoadSessionRequest {
    type Response = LoadSessionResponse;
}

impl JsonRpcMessage for LoadSessionResponse {}

impl JsonRpcIncomingMessage for LoadSessionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// NewSessionRequest
// ============================================================================

impl JsonRpcMessage for NewSessionRequest {}

impl JsonRpcOutgoingMessage for NewSessionRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/new"
    }
}

impl JsonRpcRequest for NewSessionRequest {
    type Response = NewSessionResponse;
}

impl JsonRpcMessage for NewSessionResponse {}

impl JsonRpcIncomingMessage for NewSessionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// PromptRequest
// ============================================================================

impl JsonRpcMessage for PromptRequest {}

impl JsonRpcOutgoingMessage for PromptRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/prompt"
    }
}

impl JsonRpcRequest for PromptRequest {
    type Response = PromptResponse;
}

impl JsonRpcMessage for PromptResponse {}

impl JsonRpcIncomingMessage for PromptResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// SetSessionModeRequest
// ============================================================================

impl JsonRpcMessage for SetSessionModeRequest {}

impl JsonRpcOutgoingMessage for SetSessionModeRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/set_mode"
    }
}

impl JsonRpcRequest for SetSessionModeRequest {
    type Response = SetSessionModeResponse;
}

impl JsonRpcMessage for SetSessionModeResponse {}

impl JsonRpcIncomingMessage for SetSessionModeResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}
