use agent_client_protocol::SessionNotification;

use crate::jsonrpc::JsonRpcNotification;

// Agent -> Client notifications
// These are one-way messages that agents send to clients/editors

impl JsonRpcNotification for SessionNotification {
    fn method(&self) -> String {
        "session/update".to_string()
    }

    fn into_params(self) -> impl serde::Serialize {
        self
    }
}
