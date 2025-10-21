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
    jsonrpc::{self, Handled, JsonRpcCx, JsonRpcHandler, JsonRpcRequestCx, JsonRpcResponse},
    util::{acp_to_jsonrpc_error, json_cast},
};

mod notifications;
mod requests;

/// ACP handler for agent-side messages (requests that agents receive from clients).
///
/// This implements `JsonRpcHandler` to route incoming ACP requests to your callback
/// implementation. These are the messages an agent receives: initialize, prompt,
/// new_session, etc.
pub struct AcpClientToAgentMessages<CB: AcpClientToAgentCallbacks> {
    callbacks: CB,
}

impl<CB: AcpClientToAgentCallbacks> AcpClientToAgentMessages<CB> {
    pub fn callback(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: AcpClientToAgentCallbacks> JsonRpcHandler for AcpClientToAgentMessages<CB> {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error> {
        match method {
            "initialize" => {
                self.callbacks
                    .initialize(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "session/load" => {
                self.callbacks
                    .load_session(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "session/new" => {
                self.callbacks
                    .new_session(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "session/prompt" => {
                self.callbacks
                    .prompt(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            "session/set_mode" => {
                self.callbacks
                    .set_session_mode(json_cast(params)?, response.cast())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            _ => Ok(jsonrpc::Handled::No(response)),
        }
    }

    async fn handle_notification(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &jsonrpc::JsonRpcCx,
    ) -> Result<jsonrpc::Handled<()>, jsonrpcmsg::Error> {
        match method {
            "session/cancel" => {
                self.callbacks
                    .session_cancel(json_cast(params)?, cx)
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            _ => Ok(jsonrpc::Handled::No(())),
        }
    }
}

/// Callbacks for handling agent-side ACP messages.
///
/// Implement this trait to define how your agent responds to requests from clients.
/// These are the standard ACP methods that clients call on agents.
#[allow(async_fn_in_trait)]
pub trait AcpClientToAgentCallbacks {
    /// Handle agent initialization request from client.
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: jsonrpc::JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle authentication request from client.
    async fn authenticate(
        &mut self,
        args: AuthenticateRequest,
        response: jsonrpc::JsonRpcRequestCx<AuthenticateResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session cancellation notification from client.
    async fn session_cancel(
        &mut self,
        args: CancelNotification,
        cx: &JsonRpcCx,
    ) -> Result<(), acp::Error>;

    /// Handle new session creation request from client.
    async fn new_session(
        &mut self,
        args: NewSessionRequest,
        response: jsonrpc::JsonRpcRequestCx<NewSessionResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session loading request from client.
    async fn load_session(
        &mut self,
        args: LoadSessionRequest,
        response: jsonrpc::JsonRpcRequestCx<LoadSessionResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle prompt request from client.
    async fn prompt(
        &mut self,
        args: PromptRequest,
        response: jsonrpc::JsonRpcRequestCx<PromptResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session mode change request from client.
    async fn set_session_mode(
        &mut self,
        args: SetSessionModeRequest,
        response: jsonrpc::JsonRpcRequestCx<SetSessionModeResponse>,
    ) -> Result<(), acp::Error>;
}

/// Extension trait providing convenient methods for agents to call clients.
///
/// This trait extends `JsonRpcCx` with ergonomic methods for making ACP requests
/// to clients without manually constructing request structs.
///
/// # Example
///
/// ```rust,ignore
/// // Inside an AcpAgentCallbacks implementation
/// async fn prompt(&mut self, args: PromptRequest, response: JsonRpcRequestCx<PromptResponse>) {
///     let cx = response.json_rpc_cx();
///
///     // Convenient methods instead of manual struct construction
///     let content = cx.read_text_file("src/main.rs").recv().await?;
///     cx.session_update(args.session_id, /* ... */).await?;
///
///     response.respond(PromptResponse { /* ... */ })?;
///     Ok(())
/// }
/// ```
pub trait AcpAgentToClientExt {
    /// Request permission from the user for a tool call operation.
    fn request_permission(
        &self,
        session_id: impl Into<acp::SessionId>,
        tool_call: acp::ToolCallUpdate,
        options: Vec<acp::PermissionOption>,
    ) -> JsonRpcResponse<RequestPermissionResponse>;

    /// Read content from a text file in the client's file system.
    fn read_text_file(
        &self,
        session_id: impl Into<acp::SessionId>,
        path: impl Into<std::path::PathBuf>,
    ) -> JsonRpcResponse<ReadTextFileResponse>;

    /// Write content to a text file in the client's file system.
    fn write_text_file(
        &self,
        session_id: impl Into<acp::SessionId>,
        path: impl Into<std::path::PathBuf>,
        content: impl Into<String>,
    ) -> JsonRpcResponse<WriteTextFileResponse>;

    /// Execute a command in a new terminal.
    fn create_terminal(
        &self,
        session_id: impl Into<acp::SessionId>,
        command: impl Into<String>,
        args: Vec<String>,
    ) -> JsonRpcResponse<CreateTerminalResponse>;

    /// Get the terminal output and exit status.
    fn terminal_output(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<TerminalOutputResponse>;

    /// Wait for the terminal command to exit and return its exit status.
    fn wait_for_terminal_exit(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<WaitForTerminalExitResponse>;

    /// Kill the terminal command without releasing the terminal.
    fn kill_terminal_command(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<KillTerminalCommandResponse>;

    /// Release a terminal (kills command if still running).
    fn release_terminal(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<ReleaseTerminalResponse>;

    /// Send a session notification to the client.
    fn session_update(&self, notification: SessionNotification) -> Result<(), jsonrpcmsg::Error>;
}

impl AcpAgentToClientExt for JsonRpcCx {
    fn request_permission(
        &self,
        session_id: impl Into<acp::SessionId>,
        tool_call: acp::ToolCallUpdate,
        options: Vec<acp::PermissionOption>,
    ) -> JsonRpcResponse<RequestPermissionResponse> {
        self.send_request(RequestPermissionRequest {
            session_id: session_id.into(),
            tool_call,
            options,
            meta: None,
        })
    }

    fn read_text_file(
        &self,
        session_id: impl Into<acp::SessionId>,
        path: impl Into<std::path::PathBuf>,
    ) -> JsonRpcResponse<ReadTextFileResponse> {
        self.send_request(ReadTextFileRequest {
            session_id: session_id.into(),
            path: path.into(),
            line: None,
            limit: None,
            meta: None,
        })
    }

    fn write_text_file(
        &self,
        session_id: impl Into<acp::SessionId>,
        path: impl Into<std::path::PathBuf>,
        content: impl Into<String>,
    ) -> JsonRpcResponse<WriteTextFileResponse> {
        self.send_request(WriteTextFileRequest {
            session_id: session_id.into(),
            path: path.into(),
            content: content.into(),
            meta: None,
        })
    }

    fn create_terminal(
        &self,
        session_id: impl Into<acp::SessionId>,
        command: impl Into<String>,
        args: Vec<String>,
    ) -> JsonRpcResponse<CreateTerminalResponse> {
        self.send_request(CreateTerminalRequest {
            session_id: session_id.into(),
            command: command.into(),
            args,
            env: Vec::new(),
            cwd: None,
            output_byte_limit: None,
            meta: None,
        })
    }

    fn terminal_output(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<TerminalOutputResponse> {
        self.send_request(TerminalOutputRequest {
            session_id: session_id.into(),
            terminal_id: terminal_id.into(),
            meta: None,
        })
    }

    fn wait_for_terminal_exit(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<WaitForTerminalExitResponse> {
        self.send_request(WaitForTerminalExitRequest {
            session_id: session_id.into(),
            terminal_id: terminal_id.into(),
            meta: None,
        })
    }

    fn kill_terminal_command(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<KillTerminalCommandResponse> {
        self.send_request(KillTerminalCommandRequest {
            session_id: session_id.into(),
            terminal_id: terminal_id.into(),
            meta: None,
        })
    }

    fn release_terminal(
        &self,
        session_id: impl Into<acp::SessionId>,
        terminal_id: impl Into<acp::TerminalId>,
    ) -> JsonRpcResponse<ReleaseTerminalResponse> {
        self.send_request(ReleaseTerminalRequest {
            session_id: session_id.into(),
            terminal_id: terminal_id.into(),
            meta: None,
        })
    }

    fn session_update(&self, notification: SessionNotification) -> Result<(), jsonrpcmsg::Error> {
        self.send_notification(notification)
    }
}
