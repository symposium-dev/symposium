//! Symposium ACP Proxy
//!
//! This crate provides the Symposium proxy functionality. It sits between an
//! editor and an agent, using sacp-conductor to orchestrate a dynamic chain
//! of component proxies that enrich the agent's capabilities.
//!
//! Two modes are supported:
//! - `Symposium`: Proxy mode - sits between editor and an existing agent
//! - `SymposiumAgent`: Agent mode - wraps a downstream agent
//!
//! Architecture:
//! 1. Receive Initialize request from editor
//! 2. Examine capabilities to determine what components are needed
//! 3. Build proxy chain dynamically using conductor's lazy initialization
//! 4. Forward Initialize through the chain
//! 5. Bidirectionally forward all subsequent messages

use anyhow::Result;
use sacp::link::{AgentToClient, ConductorToProxy, ProxyToConductor};
use sacp::{Component, DynComponent};
use sacp_conductor::{Conductor, McpBridgeMode};
use std::path::PathBuf;

/// Shared configuration for Symposium proxy chains.
struct SymposiumConfig {
    crate_sources_proxy: bool,
    sparkle: bool,
    trace_dir: Option<PathBuf>,
}

impl SymposiumConfig {
    fn new() -> Self {
        SymposiumConfig {
            sparkle: true,
            crate_sources_proxy: true,
            trace_dir: None,
        }
    }
}

/// Symposium in proxy mode - sits between an editor and an existing agent.
///
/// Use this when you want to add Symposium's capabilities to an existing
/// agent setup without Symposium managing the agent lifecycle.
pub struct Symposium {
    config: SymposiumConfig,
}

impl Symposium {
    pub fn new() -> Self {
        Symposium {
            config: SymposiumConfig::new(),
        }
    }

    pub fn sparkle(mut self, enable: bool) -> Self {
        self.config.sparkle = enable;
        self
    }

    pub fn crate_sources_proxy(mut self, enable: bool) -> Self {
        self.config.crate_sources_proxy = enable;
        self
    }

    /// Enable trace logging to a directory.
    /// Traces will be written as `<timestamp>.jsons` files.
    pub fn trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.trace_dir = Some(dir.into());
        self
    }

    /// Pair the symposium proxy with an agent, producing a new composite agent
    pub fn with_agent(self, agent: impl Component<AgentToClient>) -> SymposiumAgent {
        let Symposium { config } = self;
        SymposiumAgent::new(config, agent)
    }
}

impl Component<ProxyToConductor> for Symposium {
    async fn serve(self, client: impl Component<ConductorToProxy>) -> Result<(), sacp::Error> {
        tracing::debug!("Symposium::serve starting (proxy mode)");
        let Self { config } = self;

        let crate_sources_proxy = config.crate_sources_proxy;
        let sparkle = config.sparkle;
        let trace_dir = config.trace_dir;

        tracing::debug!("Creating conductor (proxy mode)");
        let mut conductor = Conductor::new_proxy(
            "symposium",
            move |init_req| async move {
                tracing::info!("Building proxy chain based on capabilities");

                let mut proxies: Vec<DynComponent<ProxyToConductor>> = vec![];

                if crate_sources_proxy {
                    proxies.push(DynComponent::new(
                        symposium_crate_sources_proxy::CrateSourcesProxy {},
                    ));
                }

                if sparkle {
                    proxies.push(DynComponent::new(sparkle::SparkleComponent::new()));
                }

                Ok((init_req, proxies))
            },
            McpBridgeMode::default(),
        );

        // Enable tracing if a directory was specified
        if let Some(dir) = trace_dir {
            std::fs::create_dir_all(&dir).map_err(sacp::Error::into_internal_error)?;
            let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let trace_path = dir.join(format!("{}.jsons", timestamp));
            conductor = conductor
                .trace_to_path(&trace_path)
                .map_err(sacp::Error::into_internal_error)?;
            tracing::info!("Tracing to {}", trace_path.display());
        }

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}

/// Symposium in agent mode - wraps a downstream agent.
///
/// Use this when Symposium should manage the agent lifecycle, e.g., when
/// building a standalone enriched agent binary.
pub struct SymposiumAgent {
    config: SymposiumConfig,
    agent: DynComponent<AgentToClient>,
}

impl SymposiumAgent {
    fn new<C: Component<AgentToClient>>(config: SymposiumConfig, agent: C) -> Self {
        SymposiumAgent {
            config,
            agent: DynComponent::new(agent),
        }
    }
}

impl Component<AgentToClient> for SymposiumAgent {
    async fn serve(
        self,
        client: impl Component<sacp::link::ClientToAgent>,
    ) -> Result<(), sacp::Error> {
        tracing::debug!("SymposiumAgent::serve starting (agent mode)");
        let Self { config, agent } = self;

        let crate_sources_proxy = config.crate_sources_proxy;
        let sparkle = config.sparkle;
        let trace_dir = config.trace_dir;

        tracing::debug!("Creating conductor (agent mode)");
        let mut conductor = Conductor::new_agent(
            "symposium",
            move |init_req| async move {
                tracing::info!("Building proxy chain based on capabilities");

                let mut proxies: Vec<DynComponent<ProxyToConductor>> = vec![];

                if crate_sources_proxy {
                    proxies.push(DynComponent::new(
                        symposium_crate_sources_proxy::CrateSourcesProxy {},
                    ));
                }

                if sparkle {
                    proxies.push(DynComponent::new(sparkle::SparkleComponent::new()));
                }

                Ok((init_req, proxies, agent))
            },
            McpBridgeMode::default(),
        );

        // Enable tracing if a directory was specified
        if let Some(dir) = trace_dir {
            std::fs::create_dir_all(&dir).map_err(sacp::Error::into_internal_error)?;
            let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let trace_path = dir.join(format!("{}.jsons", timestamp));
            conductor = conductor
                .trace_to_path(&trace_path)
                .map_err(sacp::Error::into_internal_error)?;
            tracing::info!("Tracing to {}", trace_path.display());
        }

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}
