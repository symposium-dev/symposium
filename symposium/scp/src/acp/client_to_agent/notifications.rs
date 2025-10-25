use agent_client_protocol::CancelNotification;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage};

impl JsonRpcMessage for CancelNotification {}

impl JsonRpcOutgoingMessage for CancelNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "session/cancel"
    }
}

impl JsonRpcNotification for CancelNotification {}
