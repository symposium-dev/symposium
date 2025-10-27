use agent_client_protocol::CancelNotification;
use serde::Serialize;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification};
use crate::util::json_cast;

impl JsonRpcMessage for CancelNotification {
    fn into_untyped_message(self) -> Result<crate::UntypedMessage, agent_client_protocol::Error> {
        let method = self.method().to_string();
        crate::UntypedMessage::new(&method, self)
    }

    fn method(&self) -> &str {
        "session/cancel"
    }

    fn parse_request(
        _method: &str,
        _params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        // This is a notification, not a request
        None
    }

    fn parse_notification(
        method: &str,
        params: &impl Serialize,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/cancel" {
            return None;
        }

        Some(json_cast(params))
    }
}

impl JsonRpcNotification for CancelNotification {}
