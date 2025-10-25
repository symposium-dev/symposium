use agent_client_protocol::CancelNotification;

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage};
use crate::util::json_cast;

impl JsonRpcMessage for CancelNotification {}

impl JsonRpcOutgoingMessage for CancelNotification {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, agent_client_protocol::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/cancel"
    }
}

impl JsonRpcNotification for CancelNotification {}
