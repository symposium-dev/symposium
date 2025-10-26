use agent_client_protocol::CancelNotification;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification};


impl JsonRpcMessage for CancelNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/cancel"
    }
}

impl JsonRpcNotification for CancelNotification {}
