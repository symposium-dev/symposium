//! JsonRpcRequest and JsonRpcNotification implementations for ACP enum types.
//!
//! This module implements the JSON-RPC traits for the enum types from
//! agent-client-protocol-schema that represent all possible messages:
//! - ClientRequest/AgentResponse (messages agents receive/send)
//! - ClientNotification (notifications agents receive)
//! - AgentRequest/ClientResponse (messages clients receive/send)
//! - AgentNotification (notifications clients receive)

use agent_client_protocol::{AgentNotification, AgentRequest, ClientNotification, ClientRequest};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest};

// ============================================================================
// Agent side (messages that agents receive)
// ============================================================================


impl JsonRpcMessage for ClientRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        match self {
            ClientRequest::InitializeRequest(_) => "initialize",
            ClientRequest::AuthenticateRequest(_) => "authenticate",
            ClientRequest::NewSessionRequest(_) => "session/new",
            ClientRequest::LoadSessionRequest(_) => "session/load",
            ClientRequest::SetSessionModeRequest(_) => "session/set_mode",
            ClientRequest::PromptRequest(_) => "session/prompt",
            ClientRequest::ExtMethodRequest(ext) => &ext.method,
        }
    }
}

impl JsonRpcRequest for ClientRequest {
    type Response = serde_json::Value;
}


impl JsonRpcMessage for ClientNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        match self {
            ClientNotification::CancelNotification(_) => "session/cancel",
            ClientNotification::ExtNotification(ext) => &ext.method,
        }
    }
}

impl JsonRpcNotification for ClientNotification {}

// ============================================================================
// Client side (messages that clients/editors receive)
// ============================================================================


impl JsonRpcMessage for AgentRequest {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        match self {
            AgentRequest::WriteTextFileRequest(_) => "fs/write_text_file",
            AgentRequest::ReadTextFileRequest(_) => "fs/read_text_file",
            AgentRequest::RequestPermissionRequest(_) => "session/request_permission",
            AgentRequest::CreateTerminalRequest(_) => "terminal/create",
            AgentRequest::TerminalOutputRequest(_) => "terminal/output",
            AgentRequest::ReleaseTerminalRequest(_) => "terminal/release",
            AgentRequest::WaitForTerminalExitRequest(_) => "terminal/wait_for_exit",
            AgentRequest::KillTerminalCommandRequest(_) => "terminal/kill",
            AgentRequest::ExtMethodRequest(ext) => &ext.method,
        }
    }
}

impl JsonRpcRequest for AgentRequest {
    type Response = serde_json::Value;
}


impl JsonRpcMessage for AgentNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        match self {
            AgentNotification::SessionNotification(_) => "session/update",
            AgentNotification::ExtNotification(ext) => &ext.method,
        }
    }
}

impl JsonRpcNotification for AgentNotification {}
