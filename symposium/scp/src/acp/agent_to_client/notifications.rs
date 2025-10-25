use agent_client_protocol::{self as acp, SessionNotification};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage};

// Agent -> Client notifications
// These are one-way messages that agents send to clients/editors

impl JsonRpcMessage for SessionNotification {}

impl JsonRpcOutgoingMessage for SessionNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        Ok(crate::UntypedMessage::new(
            method,
            serde_json::to_value(self).map_err(agent_client_protocol::Error::into_internal_error)?,
        ))
    }

    fn method(&self) -> &str {
        "session/update"
    }
}

impl JsonRpcNotification for SessionNotification {}
