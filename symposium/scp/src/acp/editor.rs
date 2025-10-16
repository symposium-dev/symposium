use agent_client_protocol::{
    self as acp, CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionRequest, RequestPermissionResponse,
    SessionNotification, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use crate::{
    jsonrpc::{self, JsonRpcCx, JsonRpcHandler},
    util::acp_to_jsonrpc_error,
    util::json_cast,
};

mod notifications;
mod requests;

/// ACP handler for editor-side messages (requests that editors receive from agents).
///
/// This implements `JsonRpcHandler` to route incoming ACP requests to your callback
/// implementation. These are the messages an editor receives from agents: request_permission,
/// read_text_file, write_text_file, terminal operations, and session notifications.
pub struct AcpEditor<CB: AcpEditorCallbacks> {
    callbacks: CB,
}

impl<CB: AcpEditorCallbacks> AcpEditor<CB> {
    pub fn new(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: AcpEditorCallbacks> JsonRpcHandler for AcpEditor<CB> {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: jsonrpc::JsonRpcRequestCx<jsonrpcmsg::Response>,
    ) -> Result<jsonrpc::Handled<jsonrpc::JsonRpcRequestCx<jsonrpcmsg::Response>>, jsonrpcmsg::Error>
    {
        match method {
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
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        cx: &jsonrpc::JsonRpcCx,
    ) -> Result<jsonrpc::Handled<()>, jsonrpcmsg::Error> {
        match method {
            "session/update" => {
                self.callbacks
                    .session_notification(json_cast(params)?, cx)
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(jsonrpc::Handled::Yes)
            }
            _ => Ok(jsonrpc::Handled::No(())),
        }
    }
}

/// Callbacks for handling editor-side ACP messages.
///
/// Implement this trait to define how your editor responds to requests from agents.
/// These are the messages that agents send to editors to interact with the environment.
#[allow(async_fn_in_trait)]
pub trait AcpEditorCallbacks {
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
        cx: &JsonRpcCx,
    ) -> Result<(), acp::Error>;
}
