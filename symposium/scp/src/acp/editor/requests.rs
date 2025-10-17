use agent_client_protocol::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionRequest, RequestPermissionResponse,
    TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use crate::jsonrpc::JsonRpcRequest;

// Agent -> Client requests
// These are messages that agents send to clients/editors

impl JsonRpcRequest for RequestPermissionRequest {
    type Response = RequestPermissionResponse;

    fn method(&self) -> &str {
        "session/request_permission"
    }
}

impl JsonRpcRequest for WriteTextFileRequest {
    type Response = WriteTextFileResponse;

    fn method(&self) -> &str {
        "fs/write_text_file"
    }
}

impl JsonRpcRequest for ReadTextFileRequest {
    type Response = ReadTextFileResponse;

    fn method(&self) -> &str {
        "fs/read_text_file"
    }
}

impl JsonRpcRequest for CreateTerminalRequest {
    type Response = CreateTerminalResponse;

    fn method(&self) -> &str {
        "terminal/create"
    }
}

impl JsonRpcRequest for TerminalOutputRequest {
    type Response = TerminalOutputResponse;

    fn method(&self) -> &str {
        "terminal/output"
    }
}

impl JsonRpcRequest for ReleaseTerminalRequest {
    type Response = ReleaseTerminalResponse;

    fn method(&self) -> &str {
        "terminal/release"
    }
}

impl JsonRpcRequest for WaitForTerminalExitRequest {
    type Response = WaitForTerminalExitResponse;

    fn method(&self) -> &str {
        "terminal/wait_for_exit"
    }
}

impl JsonRpcRequest for KillTerminalCommandRequest {
    type Response = KillTerminalCommandResponse;

    fn method(&self) -> &str {
        "terminal/kill"
    }
}
