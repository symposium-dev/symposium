use agent_client_protocol::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionRequest, RequestPermissionResponse,
    TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use crate::jsonrpc::{
    JsonRpcIncomingMessage, JsonRpcMessage, JsonRpcOutgoingMessage, JsonRpcRequest,
};
use crate::util::json_cast;

// Agent -> Client requests
// These are messages that agents send to clients/editors

// ============================================================================
// RequestPermissionRequest
// ============================================================================

impl JsonRpcMessage for RequestPermissionRequest {}

impl JsonRpcOutgoingMessage for RequestPermissionRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/request_permission"
    }
}

impl JsonRpcRequest for RequestPermissionRequest {
    type Response = RequestPermissionResponse;
}

impl JsonRpcMessage for RequestPermissionResponse {}

impl JsonRpcIncomingMessage for RequestPermissionResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// WriteTextFileRequest
// ============================================================================

impl JsonRpcMessage for WriteTextFileRequest {}

impl JsonRpcOutgoingMessage for WriteTextFileRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "fs/write_text_file"
    }
}

impl JsonRpcRequest for WriteTextFileRequest {
    type Response = WriteTextFileResponse;
}

impl JsonRpcMessage for WriteTextFileResponse {}

impl JsonRpcIncomingMessage for WriteTextFileResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// ReadTextFileRequest
// ============================================================================

impl JsonRpcMessage for ReadTextFileRequest {}

impl JsonRpcOutgoingMessage for ReadTextFileRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "fs/read_text_file"
    }
}

impl JsonRpcRequest for ReadTextFileRequest {
    type Response = ReadTextFileResponse;
}

impl JsonRpcMessage for ReadTextFileResponse {}

impl JsonRpcIncomingMessage for ReadTextFileResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// CreateTerminalRequest
// ============================================================================

impl JsonRpcMessage for CreateTerminalRequest {}

impl JsonRpcOutgoingMessage for CreateTerminalRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "terminal/create"
    }
}

impl JsonRpcRequest for CreateTerminalRequest {
    type Response = CreateTerminalResponse;
}

impl JsonRpcMessage for CreateTerminalResponse {}

impl JsonRpcIncomingMessage for CreateTerminalResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// TerminalOutputRequest
// ============================================================================

impl JsonRpcMessage for TerminalOutputRequest {}

impl JsonRpcOutgoingMessage for TerminalOutputRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "terminal/output"
    }
}

impl JsonRpcRequest for TerminalOutputRequest {
    type Response = TerminalOutputResponse;
}

impl JsonRpcMessage for TerminalOutputResponse {}

impl JsonRpcIncomingMessage for TerminalOutputResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// ReleaseTerminalRequest
// ============================================================================

impl JsonRpcMessage for ReleaseTerminalRequest {}

impl JsonRpcOutgoingMessage for ReleaseTerminalRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "terminal/release"
    }
}

impl JsonRpcRequest for ReleaseTerminalRequest {
    type Response = ReleaseTerminalResponse;
}

impl JsonRpcMessage for ReleaseTerminalResponse {}

impl JsonRpcIncomingMessage for ReleaseTerminalResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// WaitForTerminalExitRequest
// ============================================================================

impl JsonRpcMessage for WaitForTerminalExitRequest {}

impl JsonRpcOutgoingMessage for WaitForTerminalExitRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "terminal/wait_for_exit"
    }
}

impl JsonRpcRequest for WaitForTerminalExitRequest {
    type Response = WaitForTerminalExitResponse;
}

impl JsonRpcMessage for WaitForTerminalExitResponse {}

impl JsonRpcIncomingMessage for WaitForTerminalExitResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}

// ============================================================================
// KillTerminalCommandRequest
// ============================================================================

impl JsonRpcMessage for KillTerminalCommandRequest {}

impl JsonRpcOutgoingMessage for KillTerminalCommandRequest {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, jsonrpcmsg::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "terminal/kill"
    }
}

impl JsonRpcRequest for KillTerminalCommandRequest {
    type Response = KillTerminalCommandResponse;
}

impl JsonRpcMessage for KillTerminalCommandResponse {}

impl JsonRpcIncomingMessage for KillTerminalCommandResponse {
    fn into_json(self, _method: &str) -> Result<serde_json::Value, jsonrpcmsg::Error> {
        serde_json::to_value(self).map_err(crate::util::internal_error)
    }

    fn from_value(_method: &str, value: serde_json::Value) -> Result<Self, jsonrpcmsg::Error> {
        json_cast(&value)
    }
}
