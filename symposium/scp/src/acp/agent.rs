use agent_client_protocol::{
    self as acp, AuthenticateRequest, AuthenticateResponse, CancelNotification, InitializeRequest,
    InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SetSessionModeRequest,
    SetSessionModeResponse,
};

use crate::{
    jsonrpc::{self, JsonRpcCx, JsonRpcHandler},
    util::acp_to_jsonrpc_error,
    util::json_cast,
};

mod notifications;
mod requests;

/// ACP handler for agent-side messages (requests that agents receive from editors).
///
/// This implements `JsonRpcHandler` to route incoming ACP requests to your callback
/// implementation. These are the messages an agent receives: initialize, prompt,
/// new_session, etc.
pub struct AcpAgent<CB: AcpAgentCallbacks> {
    callbacks: CB,
}

impl<CB: AcpAgentCallbacks> AcpAgent<CB> {
    pub fn new(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: AcpAgentCallbacks> JsonRpcHandler for AcpAgent<CB> {
    async fn handle_request(
        &mut self,
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
        response: jsonrpc::JsonRpcRequestCx<jsonrpcmsg::Response>,
    ) -> Result<jsonrpc::Handled<jsonrpc::JsonRpcRequestCx<jsonrpcmsg::Response>>, jsonrpcmsg::Error>
    {
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
/// Implement this trait to define how your agent responds to requests from editors.
/// These are the standard ACP methods that editors call on agents.
#[allow(async_fn_in_trait)]
pub trait AcpAgentCallbacks {
    /// Handle agent initialization request from editor.
    async fn initialize(
        &mut self,
        args: InitializeRequest,
        response: jsonrpc::JsonRpcRequestCx<InitializeResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle authentication request from editor.
    async fn authenticate(
        &mut self,
        args: AuthenticateRequest,
        response: jsonrpc::JsonRpcRequestCx<AuthenticateResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session cancellation notification from editor.
    async fn session_cancel(
        &mut self,
        args: CancelNotification,
        cx: &JsonRpcCx,
    ) -> Result<(), acp::Error>;

    /// Handle new session creation request from editor.
    async fn new_session(
        &mut self,
        args: NewSessionRequest,
        response: jsonrpc::JsonRpcRequestCx<NewSessionResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session loading request from editor.
    async fn load_session(
        &mut self,
        args: LoadSessionRequest,
        response: jsonrpc::JsonRpcRequestCx<LoadSessionResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle prompt request from editor.
    async fn prompt(
        &mut self,
        args: PromptRequest,
        response: jsonrpc::JsonRpcRequestCx<PromptResponse>,
    ) -> Result<(), acp::Error>;

    /// Handle session mode change request from editor.
    async fn set_session_mode(
        &mut self,
        args: SetSessionModeRequest,
        response: jsonrpc::JsonRpcRequestCx<SetSessionModeResponse>,
    ) -> Result<(), acp::Error>;
}
