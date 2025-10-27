use agent_client_protocol as acp;
use jsonrpcmsg::Params;

use crate::{
    JsonRpcConnectionCx, JsonRpcNotification, JsonRpcRequest, JsonRpcRequestCx, UntypedMessage,
    util::json_cast,
};

/// Utility class for handling untyped requests.
#[must_use]
pub struct TypeRequest {
    state: Option<TypeMessageState>,
}

enum TypeMessageState {
    Unhandled(String, Option<Params>, JsonRpcRequestCx<serde_json::Value>),
    Handled(Result<(), acp::Error>),
}

impl TypeRequest {
    pub fn new(request: UntypedMessage, request_cx: JsonRpcRequestCx<serde_json::Value>) -> Self {
        let UntypedMessage { method, params } = request;
        let params: Option<Params> = json_cast(params).expect("valid params");
        Self {
            state: Some(TypeMessageState::Unhandled(method, params, request_cx)),
        }
    }

    pub async fn handle_if<R: JsonRpcRequest>(
        mut self,
        op: impl AsyncFnOnce(R, JsonRpcRequestCx<R::Response>) -> Result<(), acp::Error>,
    ) -> Self {
        self.state = Some(match self.state.take().expect("valid state") {
            TypeMessageState::Unhandled(method, params, request_cx) => {
                match R::parse_request(&method, &params) {
                    Some(Ok(request)) => {
                        TypeMessageState::Handled(op(request, request_cx.cast()).await)
                    }

                    Some(Err(err)) => TypeMessageState::Handled(request_cx.respond_with_error(err)),

                    None => TypeMessageState::Unhandled(method, params, request_cx),
                }
            }

            TypeMessageState::Handled(err) => TypeMessageState::Handled(err),
        });
        self
    }

    pub async fn otherwise(
        mut self,
        op: impl AsyncFnOnce(
            UntypedMessage,
            JsonRpcRequestCx<serde_json::Value>,
        ) -> Result<(), acp::Error>,
    ) -> Result<(), acp::Error> {
        match self.state.take().expect("valid state") {
            TypeMessageState::Unhandled(method, params, request_cx) => {
                match UntypedMessage::new(&method, params) {
                    Ok(m) => op(m, request_cx).await,
                    Err(err) => request_cx.respond_with_error(err),
                }
            }
            TypeMessageState::Handled(r) => r,
        }
    }
}

/// Utility class for handling untyped notifications.
#[must_use]
pub struct TypeNotification {
    cx: JsonRpcConnectionCx,
    state: Option<TypeNotificationState>,
}

enum TypeNotificationState {
    Unhandled(String, Option<Params>),
    Handled(Result<(), acp::Error>),
}

impl TypeNotification {
    pub fn new(request: UntypedMessage, cx: &JsonRpcConnectionCx) -> Self {
        let UntypedMessage { method, params } = request;
        let params: Option<Params> = json_cast(params).expect("valid params");
        Self {
            cx: cx.clone(),
            state: Some(TypeNotificationState::Unhandled(method, params)),
        }
    }

    pub async fn handle_if<N: JsonRpcNotification>(
        mut self,
        op: impl AsyncFnOnce(N) -> Result<(), acp::Error>,
    ) -> Self {
        self.state = Some(match self.state.take().expect("valid state") {
            TypeNotificationState::Unhandled(method, params) => {
                match N::parse_notification(&method, &params) {
                    Some(Ok(request)) => TypeNotificationState::Handled(op(request).await),

                    Some(Err(err)) => {
                        TypeNotificationState::Handled(self.cx.send_error_notification(err))
                    }

                    None => TypeNotificationState::Unhandled(method, params),
                }
            }

            TypeNotificationState::Handled(err) => TypeNotificationState::Handled(err),
        });
        self
    }

    pub async fn otherwise(
        mut self,
        op: impl AsyncFnOnce(UntypedMessage) -> Result<(), acp::Error>,
    ) -> Result<(), acp::Error> {
        match self.state.take().expect("valid state") {
            TypeNotificationState::Unhandled(method, params) => {
                match UntypedMessage::new(&method, params) {
                    Ok(m) => op(m).await,
                    Err(err) => self.cx.send_error_notification(err),
                }
            }
            TypeNotificationState::Handled(r) => r,
        }
    }
}
