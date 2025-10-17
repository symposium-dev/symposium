use agent_client_protocol::CancelNotification;

use crate::jsonrpc::JsonRpcNotification;

impl JsonRpcNotification for CancelNotification {
    const METHOD: &'static str = "session/cancel";
}
