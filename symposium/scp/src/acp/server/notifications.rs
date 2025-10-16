use agent_client_protocol::CancelNotification;

use crate::jsonrpc::JsonRpcNotification;

impl JsonRpcNotification for CancelNotification {
    fn method(&self) -> String {
        "session/cancel".to_string()
    }

    fn into_params(self) -> impl serde::Serialize {
        self
    }
}
