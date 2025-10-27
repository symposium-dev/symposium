//! Capability management for the `_meta.symposium` object in ACP messages.
//!
//! This module provides traits and types for working with capabilities stored in
//! the `_meta.symposium` field of `InitializeRequest` and `InitializeResponse`.
//!
//! # Example
//!
//! ```rust,no_run
//! use scp::{MetaCapabilityExt, Proxy};
//! # use agent_client_protocol::InitializeRequest;
//! # let init_request: InitializeRequest = unimplemented!();
//!
//! let request = init_request.add_meta_capability(Proxy);
//! if request.has_meta_capability(Proxy) {
//!     // Component has a successor in the chain
//! }
//! ```

use agent_client_protocol::{InitializeRequest, InitializeResponse};
use serde_json::json;

/// Trait for capabilities stored in the `_meta.symposium` object.
///
/// Capabilities are key-value pairs that signal features or context to components
/// in the proxy chain. Implement this trait to define new capabilities.
pub trait MetaCapability {
    /// The key name in the `_meta.symposium` object (e.g., "proxy", "mcp_acp_transport")
    fn key(&self) -> &'static str;

    /// The value to set when adding this capability (defaults to `true`)
    fn value(&self) -> serde_json::Value {
        serde_json::Value::Bool(true)
    }
}

/// The proxy capability - indicates a component has a successor in the proxy chain.
///
/// When present in `_meta.symposium.proxy`, signals that the component should use
/// the `_proxy/successor/*` protocol to communicate with its successor.
pub struct Proxy;

impl MetaCapability for Proxy {
    fn key(&self) -> &'static str {
        "proxy"
    }
}

/// The mcp_acp_transport capability - indicates support for MCP-over-ACP bridging.
///
/// When present in `_meta.symposium.mcp_acp_transport`, signals that the agent
/// supports having MCP servers with `acp:UUID` transport proxied through the conductor.
pub struct McpAcpTransport;

impl MetaCapability for McpAcpTransport {
    fn key(&self) -> &'static str {
        "mcp_acp_transport"
    }
}

/// Extension trait for checking and modifying capabilities in `InitializeRequest`.
pub trait MetaCapabilityExt {
    /// Check if a capability is present in `_meta.symposium`
    fn has_meta_capability(&self, capability: impl MetaCapability) -> bool;

    /// Add a capability to `_meta.symposium`, creating the structure if needed
    fn add_meta_capability(self, capability: impl MetaCapability) -> Self;

    /// Remove a capability from `_meta.symposium` if present
    fn remove_meta_capability(self, capability: impl MetaCapability) -> Self;
}

impl MetaCapabilityExt for InitializeRequest {
    fn has_meta_capability(&self, capability: impl MetaCapability) -> bool {
        self.client_capabilities
            .meta
            .as_ref()
            .and_then(|meta| meta.get("symposium"))
            .and_then(|symposium| symposium.get(capability.key()))
            .is_some()
    }

    fn add_meta_capability(mut self, capability: impl MetaCapability) -> Self {
        let mut meta = self.client_capabilities.meta.take().unwrap_or(json!({}));

        if let Some(obj) = meta.as_object_mut() {
            let symposium = obj.entry("symposium").or_insert_with(|| json!({}));

            if let Some(symposium_obj) = symposium.as_object_mut() {
                symposium_obj.insert("version".to_string(), json!("1.0"));
                symposium_obj.insert(capability.key().to_string(), capability.value());
            }
        }

        self.client_capabilities.meta = Some(meta);
        self
    }

    fn remove_meta_capability(mut self, capability: impl MetaCapability) -> Self {
        if let Some(ref mut meta) = self.client_capabilities.meta {
            if let Some(obj) = meta.as_object_mut() {
                if let Some(symposium) = obj.get_mut("symposium") {
                    if let Some(symposium_obj) = symposium.as_object_mut() {
                        symposium_obj.remove(capability.key());
                    }
                }
            }
        }
        self
    }
}

impl MetaCapabilityExt for InitializeResponse {
    fn has_meta_capability(&self, capability: impl MetaCapability) -> bool {
        self.agent_capabilities
            .meta
            .as_ref()
            .and_then(|meta| meta.get("symposium"))
            .and_then(|symposium| symposium.get(capability.key()))
            .is_some()
    }

    fn add_meta_capability(mut self, capability: impl MetaCapability) -> Self {
        let mut meta = self.agent_capabilities.meta.take().unwrap_or(json!({}));

        if let Some(obj) = meta.as_object_mut() {
            let symposium = obj.entry("symposium").or_insert_with(|| json!({}));

            if let Some(symposium_obj) = symposium.as_object_mut() {
                symposium_obj.insert("version".to_string(), json!("1.0"));
                symposium_obj.insert(capability.key().to_string(), capability.value());
            }
        }

        self.agent_capabilities.meta = Some(meta);
        self
    }

    fn remove_meta_capability(mut self, capability: impl MetaCapability) -> Self {
        if let Some(ref mut meta) = self.agent_capabilities.meta {
            if let Some(obj) = meta.as_object_mut() {
                if let Some(symposium) = obj.get_mut("symposium") {
                    if let Some(symposium_obj) = symposium.as_object_mut() {
                        symposium_obj.remove(capability.key());
                    }
                }
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_add_proxy_capability() {
        let request = InitializeRequest {
            protocol_version: agent_client_protocol::VERSION,
            client_capabilities: agent_client_protocol::ClientCapabilities::default(),
            meta: None,
        };

        let request = request.add_meta_capability(Proxy);

        assert!(request.has_meta_capability(Proxy));
        assert_eq!(
            request.client_capabilities.meta.as_ref().unwrap()["symposium"]["proxy"],
            json!(true)
        );
    }

    #[test]
    fn test_remove_proxy_capability() {
        let mut client_capabilities = agent_client_protocol::ClientCapabilities::default();
        client_capabilities.meta = Some(json!({
            "symposium": {
                "version": "1.0",
                "proxy": true
            }
        }));

        let request = InitializeRequest {
            protocol_version: agent_client_protocol::VERSION,
            client_capabilities,
            meta: None,
        };

        let request = request.remove_meta_capability(Proxy);

        assert!(!request.has_meta_capability(Proxy));
    }

    #[test]
    fn test_has_proxy_capability() {
        let mut client_capabilities = agent_client_protocol::ClientCapabilities::default();
        client_capabilities.meta = Some(json!({
            "symposium": {
                "proxy": true
            }
        }));

        let request = InitializeRequest {
            protocol_version: agent_client_protocol::VERSION,
            client_capabilities,
            meta: None,
        };

        assert!(request.has_meta_capability(Proxy));
        assert!(!request.has_meta_capability(McpAcpTransport));
    }

    #[test]
    fn test_response_capabilities() {
        let response = InitializeResponse {
            protocol_version: agent_client_protocol::VERSION,
            agent_capabilities: agent_client_protocol::AgentCapabilities::default(),
            auth_methods: vec![],
            meta: None,
        };

        let response = response.add_meta_capability(McpAcpTransport);

        assert!(response.has_meta_capability(McpAcpTransport));
        assert_eq!(
            response.agent_capabilities.meta.as_ref().unwrap()["symposium"]["mcp_acp_transport"],
            json!(true)
        );
    }
}
