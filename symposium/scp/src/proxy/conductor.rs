use crate::{
    JsonRpcNotificationCx,
    jsonrpc::{Handled, JsonRpcConnectionCx, JsonRpcHandler, JsonRpcRequestCx},
    proxy::{ToSuccessorNotification, ToSuccessorRequest},
    util::{acp_to_jsonrpc_error, json_cast},
};
use agent_client_protocol as acp;

/// Callbacks for the conductor who receives requests from proxies to forward messages over to their successor.
#[allow(async_fn_in_trait)]
pub trait ConductorCallbacks {
    /// Name of the method to be invoked
    /// Parameters for the method invocation
    async fn successor_send_request(
        &mut self,
        args: ToSuccessorRequest<serde_json::Value>,
        response: JsonRpcRequestCx<serde_json::Value>,
    ) -> Result<(), acp::Error>;

    /// Name of the method to be invoked
    /// Parameters for the method invocation
    async fn successor_send_notification(
        &mut self,
        args: ToSuccessorNotification<serde_json::Value>,
        cx: &JsonRpcConnectionCx,
    ) -> Result<(), acp::Error>;
}

/// Message handler for messages targeting the conductor.
pub struct ProxyToConductorMessages<CB: ConductorCallbacks> {
    callbacks: CB,
}

impl<CB: ConductorCallbacks> ProxyToConductorMessages<CB> {
    /// Create new handler that invokes `callbacks` when requests from proxies are received.
    pub fn callback(callbacks: CB) -> Self {
        Self { callbacks }
    }
}

impl<CB: ConductorCallbacks> JsonRpcHandler for ProxyToConductorMessages<CB> {
    async fn handle_request(
        &mut self,
        cx: JsonRpcRequestCx<serde_json::Value>,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::jsonrpc::Handled<JsonRpcRequestCx<serde_json::Value>>, jsonrpcmsg::Error>
    {
        match cx.method() {
            "_proxy/successor/send/request" => {
                // Proxy is requesting us to send this message to their successor.
                self.callbacks
                    .successor_send_request(json_cast(params)?, cx.parse_from_json())
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(Handled::Yes)
            }

            _ => Ok(Handled::No(cx)),
        }
    }

    async fn handle_notification(
        &mut self,
        cx: JsonRpcNotificationCx,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Result<crate::jsonrpc::Handled<JsonRpcNotificationCx>, jsonrpcmsg::Error> {
        match cx.method() {
            "_proxy/successor/send/notification" => {
                // Proxy is requesting us to send this message to their successor.
                self.callbacks
                    .successor_send_notification(json_cast(params)?, &cx)
                    .await
                    .map_err(acp_to_jsonrpc_error)?;
                Ok(Handled::Yes)
            }

            _ => Ok(Handled::No(cx)),
        }
    }
}
