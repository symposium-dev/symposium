//! Symposium ACP Proxy
//!
//! This crate provides the Symposium proxy functionality. It sits between an
//! editor and an agent, using sacp-conductor to orchestrate a dynamic chain
//! of component proxies that enrich the agent's capabilities.
//!
//! Architecture:
//! 1. Receive Initialize request from editor
//! 2. Examine capabilities to determine what components are needed
//! 3. Build proxy chain dynamically using conductor's lazy initialization
//! 4. Forward Initialize through the chain
//! 5. Bidirectionally forward all subsequent messages

use anyhow::Result;
use sacp::Component;
use sacp_conductor::{Conductor, McpBridgeMode};

pub struct Symposium {
    crate_sources_proxy: bool,
    sparkle: bool,
}

impl Symposium {
    pub fn new() -> Self {
        Symposium {
            sparkle: true,
            crate_sources_proxy: true,
        }
    }

    pub fn sparkle(mut self, enable: bool) -> Self {
        self.sparkle = enable;
        self
    }

    pub fn crate_sources_proxy(mut self, enable: bool) -> Self {
        self.crate_sources_proxy = enable;
        self
    }
}

impl sacp::Component for Symposium {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        let Self {
            crate_sources_proxy,
            sparkle,
        } = self;
        Conductor::new(
            "symposium".to_string(),
            move |init_req| async move {
                tracing::info!("Building proxy chain based on capabilities");

                // TODO: Examine init_req.capabilities to determine what's needed

                let mut components = vec![];

                if crate_sources_proxy {
                    components.push(sacp::DynComponent::new(
                        symposium_crate_sources_proxy::CrateSourcesProxy {},
                    ));
                }

                if sparkle {
                    components.push(sacp::DynComponent::new(sparkle::SparkleComponent::new()));
                }

                // TODO: Add more components based on capabilities
                // - Check for IDE operation capabilities
                // - Spawn ide-ops adapter if missing
                // - Spawn ide-ops component to provide MCP tools

                Ok((init_req, components))
            },
            McpBridgeMode::default(),
        )
        .run(client)
        .await
    }
}
