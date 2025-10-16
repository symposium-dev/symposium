use agent_client_protocol::{
    AuthenticateRequest, AuthenticateResponse, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SetSessionModeRequest, SetSessionModeResponse,
};

use crate::jsonrpc::JsonRpcRequest;

impl JsonRpcRequest for InitializeRequest {
    type Response = InitializeResponse;

    const METHOD: &'static str = "initialize";
}

impl JsonRpcRequest for AuthenticateRequest {
    type Response = AuthenticateResponse;

    const METHOD: &'static str = "authenticate";
}

impl JsonRpcRequest for LoadSessionRequest {
    type Response = LoadSessionResponse;

    const METHOD: &'static str = "session/load";
}

impl JsonRpcRequest for NewSessionRequest {
    type Response = NewSessionResponse;

    const METHOD: &'static str = "session/new";
}

impl JsonRpcRequest for PromptRequest {
    type Response = PromptResponse;

    const METHOD: &'static str = "session/prompt";
}

impl JsonRpcRequest for SetSessionModeRequest {
    type Response = SetSessionModeResponse;

    const METHOD: &'static str = "session/set_mode";
}
