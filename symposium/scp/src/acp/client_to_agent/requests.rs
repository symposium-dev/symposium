use agent_client_protocol::{
    AuthenticateRequest, AuthenticateResponse, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SetSessionModeRequest, SetSessionModeResponse,
};

use crate::jsonrpc::JsonRpcRequest;

impl JsonRpcRequest for InitializeRequest {
    type Response = InitializeResponse;

    fn method(&self) -> &str {
        "initialize"
    }
}

impl JsonRpcRequest for AuthenticateRequest {
    type Response = AuthenticateResponse;

    fn method(&self) -> &str {
        "authenticate"
    }
}

impl JsonRpcRequest for LoadSessionRequest {
    type Response = LoadSessionResponse;

    fn method(&self) -> &str {
        "session/load"
    }
}

impl JsonRpcRequest for NewSessionRequest {
    type Response = NewSessionResponse;

    fn method(&self) -> &str {
        "session/new"
    }
}

impl JsonRpcRequest for PromptRequest {
    type Response = PromptResponse;

    fn method(&self) -> &str {
        "session/prompt"
    }
}

impl JsonRpcRequest for SetSessionModeRequest {
    type Response = SetSessionModeResponse;

    fn method(&self) -> &str {
        "session/set_mode"
    }
}
