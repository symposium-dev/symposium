//! JsonRpcRequest and JsonRpcNotification implementations for ACP enum types.
//!
//! This module implements the JSON-RPC traits for the enum types from
//! agent-client-protocol-schema that represent all possible messages:
//! - ClientRequest/AgentResponse (messages agents receive/send)
//! - ClientNotification (notifications agents receive)
//! - AgentRequest/ClientResponse (messages clients receive/send)
//! - AgentNotification (notifications clients receive)

use agent_client_protocol::{AgentNotification, AgentRequest, ClientNotification, ClientRequest};
use serde::Serialize;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest};
use crate::util::json_cast;

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

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        let result = match method {
            "initialize" => json_cast(params).map(ClientRequest::InitializeRequest),
            "authenticate" => json_cast(params).map(ClientRequest::AuthenticateRequest),
            "session/new" => json_cast(params).map(ClientRequest::NewSessionRequest),
            "session/load" => json_cast(params).map(ClientRequest::LoadSessionRequest),
            "session/set_mode" => json_cast(params).map(ClientRequest::SetSessionModeRequest),
            "session/prompt" => json_cast(params).map(ClientRequest::PromptRequest),
            _ => {
                // Check for extension methods (prefixed with underscore)
                if let Some(custom_method) = method.strip_prefix('_') {
                    json_cast(params).map(|ext_req: agent_client_protocol::ExtRequest| {
                        ClientRequest::ExtMethodRequest(agent_client_protocol::ExtRequest {
                            method: custom_method.to_string().into(),
                            params: ext_req.params,
                        })
                    })
                } else {
                    return None;
                }
            }
        };

        Some(result)
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // ClientRequest is for requests only, not notifications
        None
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

    fn parse_request(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // ClientNotification is for notifications only, not requests
        None
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        let result = match method {
            "session/cancel" => json_cast(params).map(ClientNotification::CancelNotification),
            _ => {
                // Check for extension notifications (prefixed with underscore)
                if let Some(custom_method) = method.strip_prefix('_') {
                    json_cast(params).map(|ext_notif: agent_client_protocol::ExtNotification| {
                        ClientNotification::ExtNotification(
                            agent_client_protocol::ExtNotification {
                                method: custom_method.to_string().into(),
                                params: ext_notif.params,
                            },
                        )
                    })
                } else {
                    return None;
                }
            }
        };

        Some(result)
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

    fn parse_request(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        let result = match method {
            "fs/write_text_file" => json_cast(params).map(AgentRequest::WriteTextFileRequest),
            "fs/read_text_file" => json_cast(params).map(AgentRequest::ReadTextFileRequest),
            "session/request_permission" => {
                json_cast(params).map(AgentRequest::RequestPermissionRequest)
            }
            "terminal/create" => json_cast(params).map(AgentRequest::CreateTerminalRequest),
            "terminal/output" => json_cast(params).map(AgentRequest::TerminalOutputRequest),
            "terminal/release" => json_cast(params).map(AgentRequest::ReleaseTerminalRequest),
            "terminal/wait_for_exit" => {
                json_cast(params).map(AgentRequest::WaitForTerminalExitRequest)
            }
            "terminal/kill" => json_cast(params).map(AgentRequest::KillTerminalCommandRequest),
            _ => {
                // Check for extension methods (prefixed with underscore)
                if let Some(custom_method) = method.strip_prefix('_') {
                    json_cast(params).map(|ext_req: agent_client_protocol::ExtRequest| {
                        AgentRequest::ExtMethodRequest(agent_client_protocol::ExtRequest {
                            method: custom_method.to_string().into(),
                            params: ext_req.params,
                        })
                    })
                } else {
                    return None;
                }
            }
        };

        Some(result)
    }

    fn parse_notification(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // AgentRequest is for requests only, not notifications
        None
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

    fn parse_request(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // AgentNotification is for notifications only, not requests
        None
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        let result = match method {
            "session/update" => json_cast(params).map(AgentNotification::SessionNotification),
            _ => {
                // Check for extension notifications (prefixed with underscore)
                if let Some(custom_method) = method.strip_prefix('_') {
                    json_cast(params).map(|ext_notif: agent_client_protocol::ExtNotification| {
                        AgentNotification::ExtNotification(agent_client_protocol::ExtNotification {
                            method: custom_method.to_string().into(),
                            params: ext_notif.params,
                        })
                    })
                } else {
                    return None;
                }
            }
        };

        Some(result)
    }
}

impl JsonRpcNotification for AgentNotification {}
