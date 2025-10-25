use agent_client_protocol::{self as acp, SessionNotification};

use crate::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcOutgoingMessage};
use crate::util::json_cast;

// Agent -> Client notifications
// These are one-way messages that agents send to clients/editors

impl JsonRpcMessage for SessionNotification {}

impl JsonRpcOutgoingMessage for SessionNotification {
    fn params(self) -> Result<Option<jsonrpcmsg::Params>, acp::Error> {
        json_cast(self)
    }

    fn method(&self) -> &str {
        "session/update"
    }
}

impl JsonRpcNotification for SessionNotification {}
