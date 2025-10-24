use std::error::Error;

use agent_client_protocol::{
    self as acp, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    CreateTerminalRequest, CreateTerminalResponse, InitializeRequest, InitializeResponse,
    KillTerminalCommandRequest, KillTerminalCommandResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification,
    SetSessionModeRequest, SetSessionModeResponse, TerminalOutputRequest, TerminalOutputResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};

use crate::{
    JsonRpcNotificationCx,
    jsonrpc::{
        self, Handled, JsonRpcConnectionCx, JsonRpcHandler, JsonRpcRequestCx, JsonRpcResponse,
    },
    util::{acp_to_jsonrpc_error, json_cast},
};

mod notifications;
mod requests;

/// Messages that editors receive from agents via the ACP protocol.
/// Unifies both requests (which expect responses) and notifications (fire-and-forget).
pub enum AcpAgentToClientMessage {
    /// A request from the agent that expects a response.
    Request(acp::AgentRequest, JsonRpcRequestCx<serde_json::Value>),
    /// A notification from the agent (no response expected).
    Notification(acp::AgentNotification, JsonRpcConnectionCx),
}

/// ACP handler for editor-side messages (requests that editors receive from agents).
///
/// This implements `JsonRpcHandler` to route incoming ACP requests to your callback
/// implementation. These are the messages an editor receives from agents: request_permission,
/// read_text_file, write_text_file, terminal operations, and session notifications.
pub struct AcpAgentToClientMessages<CB: AcpAgentToClientCallbacks> {
    callbacks: CB,
}

impl<CB: AcpAgentToClientCallbacks> AcpAgentToClientMessages<CB> {
    pub fn callback(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: AcpAgentToClientCallbacks> JsonRpcHandler for AcpAgentToClientMessages<CB> {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        match cx.method() {
            "session/request_permission" => {
                self.callbacks
                    .request_permission(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "fs/write_text_file" => {
                self.callbacks
                    .write_text_file(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "fs/read_text_file" => {
                self.callbacks
                    .read_text_file(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "terminal/create" => {
                self.callbacks
                    .create_terminal(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "terminal/output" => {
                self.callbacks
                    .terminal_output(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "terminal/release" => {
                self.callbacks
                    .release_terminal(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "terminal/wait_for_exit" => {
                self.callbacks
                    .wait_for_terminal_exit(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "terminal/kill" => {
                self.callbacks
                    .kill_terminal_command(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            _ => Ok(jsonrpc::Handled::No(response)),
        }
    }

    async fn handle_notification(
        &mut self,
        cx: jsonrpc::JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<jsonrpc::Handled<jsonrpc::JsonRpcNotificationCx>, jsonrpcmsg::Error> {
        match cx.method() {
            "session/update" => {
                self.callbacks
                    .session_notification(json_cast(params)?, cx)
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            _ => Ok(jsonrpc::Handled::No(cx)),
        }
    }
}

/// Callbacks for handling editor-side ACP messages.
///
/// Implement this trait to define how your editor responds to requests from agents.
/// These are the messages that agents send to editors to interact with the environment.
#[allow(async_fn_in_trait)]
pub trait AcpAgentToClientCallbacks {
    /// Handle permission request from agent.
    async fn request_permission(
        &mut self,
        args: RequestPermissionRequest,
        response: jsonrpc::JsonRpcRequestCx<RequestPermissionResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle file write request from agent.
    async fn write_text_file(
        &mut self,
        args: WriteTextFileRequest,
        response: jsonrpc::JsonRpcRequestCx<WriteTextFileResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle file read request from agent.
    async fn read_text_file(
        &mut self,
        args: ReadTextFileRequest,
        response: jsonrpc::JsonRpcRequestCx<ReadTextFileResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle terminal creation request from agent.
    async fn create_terminal(
        &mut self,
        args: CreateTerminalRequest,
        response: jsonrpc::JsonRpcRequestCx<CreateTerminalResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle terminal output request from agent.
    async fn terminal_output(
        &mut self,
        args: TerminalOutputRequest,
        response: jsonrpc::JsonRpcRequestCx<TerminalOutputResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle terminal release request from agent.
    async fn release_terminal(
        &mut self,
        args: ReleaseTerminalRequest,
        response: jsonrpc::JsonRpcRequestCx<ReleaseTerminalResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle wait for terminal exit request from agent.
    async fn wait_for_terminal_exit(
        &mut self,
        args: WaitForTerminalExitRequest,
        response: jsonrpc::JsonRpcRequestCx<WaitForTerminalExitResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle kill terminal command request from agent.
    async fn kill_terminal_command(
        &mut self,
        args: KillTerminalCommandRequest,
        response: jsonrpc::JsonRpcRequestCx<KillTerminalCommandResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session notification from agent.
    async fn session_notification(
        &mut self,
        args: SessionNotification,
        cx: JsonRpcNotificationCx,
    ) -> Result<(), acp::Error>;
}

/// Extension trait providing convenient methods for editors to call agents.
///
/// This trait extends `JsonRpcCx` with ergonomic methods for making ACP requests
/// to agents without manually constructing request structs.
///
/// # Example
///
/// ```rust,ignore
/// // Inside an AcpClientCallbacks implementation
/// async fn read_text_file(&mut self, args: ReadTextFileRequest, response: JsonRpcRequestCx<ReadTextFileResponse>) {
///     let cx = response.json_rpc_cx();
///
///     // Convenient methods instead of manual struct construction
///     let result = cx.prompt(args.session_id, prompt).recv().await?;
///
///     response.respond(ReadTextFileResponse { /* ... */ })?;
///     Ok(())
/// }
/// ```
pub trait AcpClientToAgentExt {
    /// Initialize the agent connection.
    fn initialize(
        &self,
        protocol_version: acp::ProtocolVersion,
        client_capabilities: acp::ClientCapabilities,
    ) -> JsonRpcResponse<InitializeResponse>;

    /// Authenticate with the agent.
    fn authenticate(&self, method_id: acp::AuthMethodId) -> JsonRpcResponse<AuthenticateResponse>;

    /// Create a new agent session.
    fn new_session(
        &self,
        mcp_servers: Vec<acp::McpServer>,
        cwd: std::path::PathBuf,
    ) -> JsonRpcResponse<NewSessionResponse>;

    /// Load an existing agent session.
    fn load_session(
        &self,
        session_id: impl Into<acp::SessionId>,
        mcp_servers: Vec<acp::McpServer>,
        cwd: std::path::PathBuf,
    ) -> JsonRpcResponse<LoadSessionResponse>;

    /// Send a prompt to the agent.
    fn prompt(
        &self,
        session_id: impl Into<acp::SessionId>,
        prompt: impl IntoIterator<Item = acp::ContentBlock>,
    ) -> JsonRpcResponse<PromptResponse>;

    /// Set the session mode.
    fn set_session_mode(
        &self,
        session_id: impl Into<acp::SessionId>,
        mode_id: acp::SessionModeId,
    ) -> JsonRpcResponse<SetSessionModeResponse>;

    /// Cancel an in-progress request.
    fn session_cancel(
        &self,
        session_id: impl Into<acp::SessionId>,
    ) -> Result<(), jsonrpcmsg::Error>;
}

impl AcpClientToAgentExt for JsonRpcConnectionCx {
    fn initialize(
        &self,
        protocol_version: acp::ProtocolVersion,
        client_capabilities: acp::ClientCapabilities,
    ) -> JsonRpcResponse<InitializeResponse> {
        self.send_request(InitializeRequest {
            protocol_version,
            client_capabilities,
            meta: None,
        })
    }

    fn authenticate(&self, method_id: acp::AuthMethodId) -> JsonRpcResponse<AuthenticateResponse> {
        self.send_request(AuthenticateRequest {
            method_id,
            meta: None,
        })
    }

    fn new_session(
        &self,
        mcp_servers: Vec<acp::McpServer>,
        cwd: std::path::PathBuf,
    ) -> JsonRpcResponse<NewSessionResponse> {
        self.send_request(NewSessionRequest {
            mcp_servers,
            cwd,
            meta: None,
        })
    }

    fn load_session(
        &self,
        session_id: impl Into<acp::SessionId>,
        mcp_servers: Vec<acp::McpServer>,
        cwd: std::path::PathBuf,
    ) -> JsonRpcResponse<LoadSessionResponse> {
        self.send_request(LoadSessionRequest {
            session_id: session_id.into(),
            mcp_servers,
            cwd,
            meta: None,
        })
    }

    fn prompt(
        &self,
        session_id: impl Into<acp::SessionId>,
        prompt: impl IntoIterator<Item = acp::ContentBlock>,
    ) -> JsonRpcResponse<PromptResponse> {
        self.send_request(PromptRequest {
            session_id: session_id.into(),
            prompt: prompt.into_iter().collect(),
            meta: None,
        })
    }

    fn set_session_mode(
        &self,
        session_id: impl Into<acp::SessionId>,
        mode_id: acp::SessionModeId,
    ) -> JsonRpcResponse<SetSessionModeResponse> {
        self.send_request(SetSessionModeRequest {
            session_id: session_id.into(),
            mode_id,
            meta: None,
        })
    }

    fn session_cancel(
        &self,
        session_id: impl Into<acp::SessionId>,
    ) -> Result<(), jsonrpcmsg::Error> {
        self.send_notification(CancelNotification {
            session_id: session_id.into(),
            meta: None,
        })
    }
}

impl<TX, E> AcpAgentToClientMessages<AcpAgentToClientSendTo<TX, E>>
where
    TX: AsyncFnMut(AcpAgentToClientMessage) -> Result<(), E>,
    E: Error,
{
    pub fn send_to(tx: TX) -> Self {
        Self::callback(AcpAgentToClientSendTo { tx })
    }
}

pub struct AcpAgentToClientSendTo<TX, E>
where
    TX: AsyncFnMut(AcpAgentToClientMessage) -> Result<(), E>,
    E: Error,
{
    tx: TX,
}

impl<TX, E> AcpAgentToClientSendTo<TX, E>
where
    TX: AsyncFnMut(AcpAgentToClientMessage) -> Result<(), E>,
    E: Error,
{
}

impl<TX, E> AcpAgentToClientCallbacks for AcpAgentToClientSendTo<TX, E>
where
    TX: AsyncFnMut(AcpAgentToClientMessage) -> Result<(), E>,
    E: Error,
{
    async fn request_permission(
        &mut self,
        args: RequestPermissionRequest,
        response: jsonrpc::JsonRpcRequestCx<RequestPermissionResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::RequestPermissionRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn write_text_file(
        &mut self,
        args: WriteTextFileRequest,
        response: jsonrpc::JsonRpcRequestCx<WriteTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::WriteTextFileRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn read_text_file(
        &mut self,
        args: ReadTextFileRequest,
        response: jsonrpc::JsonRpcRequestCx<ReadTextFileResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::ReadTextFileRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn create_terminal(
        &mut self,
        args: CreateTerminalRequest,
        response: jsonrpc::JsonRpcRequestCx<CreateTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::CreateTerminalRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn terminal_output(
        &mut self,
        args: TerminalOutputRequest,
        response: jsonrpc::JsonRpcRequestCx<TerminalOutputResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::TerminalOutputRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn release_terminal(
        &mut self,
        args: ReleaseTerminalRequest,
        response: jsonrpc::JsonRpcRequestCx<ReleaseTerminalResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::ReleaseTerminalRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn wait_for_terminal_exit(
        &mut self,
        args: WaitForTerminalExitRequest,
        response: jsonrpc::JsonRpcRequestCx<WaitForTerminalExitResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Request(
            acp::AgentRequest::WaitForTerminalExitRequest(args),
            response.erase_to_json(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }

    async fn kill_terminal_command(
        &mut self,
        _args: KillTerminalCommandRequest,
        _response: jsonrpc::JsonRpcRequestCx<KillTerminalCommandResponse>,
    ) -> Result<(), agent_client_protocol::Error> {
        panic!("FIXME: kill_terminal_command is missing entries in the enum")
        // (self.tx)(AcpClientMessage::Request(
        //     acp::AgentRequest::KillTerminalCommandRequest(args),
        //     response.map(
        //         move |client_response: ClientResponse| match client_response {
        //             ClientResponse::KillTerminalCommandResponse(kill_terminal_command_response) => {
        //                 Ok(kill_terminal_command_response)
        //             }
        //             _ => Err(jsonrpcmsg::Error::internal_error()),
        //         },
        //         move |error| Err(error),
        //     ),
        // ))
        // .await
        // .map_err(acp::Error::into_internal_error)
    }

    async fn session_notification(
        &mut self,
        args: SessionNotification,
        cx: JsonRpcNotificationCx,
    ) -> Result<(), agent_client_protocol::Error> {
        (self.tx)(AcpAgentToClientMessage::Notification(
            acp::AgentNotification::SessionNotification(args),
            cx.clone(),
        ))
        .await
        .map_err(acp::Error::into_internal_error)
    }
}
