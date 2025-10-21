use agent_client_protocol::CancelNotification;

use crate::jsonrpc::JsonRpcNotification;

impl JsonRpcNotification for CancelNotification {
    fn method(&self) -> &str {
        "session/cancel"
    }
}
