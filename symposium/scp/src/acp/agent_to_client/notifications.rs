use agent_client_protocol::{self as acp, SessionNotification};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification};

// Agent -> Client notifications
// These are one-way messages that agents send to clients/editors


impl JsonRpcMessage for SessionNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, acp::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/update"
    }
}

impl JsonRpcNotification for SessionNotification {}
