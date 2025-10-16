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

    const METHOD: &'static str = "session/request_permission";
}

impl JsonRpcRequest for WriteTextFileRequest {
    type Response = WriteTextFileResponse;

    const METHOD: &'static str = "fs/write_text_file";
}

impl JsonRpcRequest for ReadTextFileRequest {
    type Response = ReadTextFileResponse;

    const METHOD: &'static str = "fs/read_text_file";
}

impl JsonRpcRequest for CreateTerminalRequest {
    type Response = CreateTerminalResponse;

    const METHOD: &'static str = "terminal/create";
}

impl JsonRpcRequest for TerminalOutputRequest {
    type Response = TerminalOutputResponse;

    const METHOD: &'static str = "terminal/output";
}

impl JsonRpcRequest for ReleaseTerminalRequest {
    type Response = ReleaseTerminalResponse;

    const METHOD: &'static str = "terminal/release";
}

impl JsonRpcRequest for WaitForTerminalExitRequest {
    type Response = WaitForTerminalExitResponse;

    const METHOD: &'static str = "terminal/wait_for_exit";
}

impl JsonRpcRequest for KillTerminalCommandRequest {
    type Response = KillTerminalCommandResponse;

    const METHOD: &'static str = "terminal/kill";
}
