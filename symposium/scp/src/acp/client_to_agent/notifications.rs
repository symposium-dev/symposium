use agent_client_protocol::CancelNotification;

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

    fn parse_notification(
        method: &str,
        params: &Option<jsonrpcmsg::Params>,
    ) -> Option<Result<Self, agent_client_protocol::Error>> {
        if method != "session/cancel" {
            return None;
        }

        let params = match params {
            Some(p) => p,
            None => return Some(Err(agent_client_protocol::Error::invalid_params())),
        };

        Some(json_cast(params))
    }
}

impl JsonRpcNotification for CancelNotification {}
