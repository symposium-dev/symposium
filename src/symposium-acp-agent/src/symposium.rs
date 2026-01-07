//! Symposium proxy chain orchestration
//!
//! This module provides the core Symposium functionality - building and running
//! proxy chains that enrich agent capabilities.
//!
//! Two modes are supported:
//! - `Symposium`: Proxy mode - sits between editor and an existing agent
//! - `SymposiumAgent`: Agent mode - wraps a downstream agent

use sacp::link::{AgentToClient, ConductorToProxy, ProxyToConductor};
use sacp::{Component, DynComponent};
use sacp_conductor::{Conductor, McpBridgeMode};
use std::path::PathBuf;

/// Known proxy/extension names that can be configured.
pub const KNOWN_PROXIES: &[&str] = &["sparkle", "ferris", "cargo"];

/// Shared configuration for Symposium proxy chains.
#[derive(Clone)]
pub struct SymposiumConfig {
    /// Ordered list of proxy names to include in the chain.
    proxy_names: Vec<String>,
    trace_dir: Option<PathBuf>,
}

impl SymposiumConfig {
    /// Create with no proxies.
    pub fn new() -> Self {
        SymposiumConfig {
            proxy_names: Vec::new(),
            trace_dir: None,
        }
    }

    /// Create from a list of proxy names.
    pub fn from_proxy_names(names: Vec<String>) -> Self {
        SymposiumConfig {
            proxy_names: names,
            trace_dir: None,
        }
    }

    /// Set the trace directory.
    pub fn trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.trace_dir = Some(dir.into());
        self
    }

    /// Build proxy components from the configured names, preserving order.
    fn build_proxies(&self) -> Vec<DynComponent<ProxyToConductor>> {
        let mut proxies: Vec<DynComponent<ProxyToConductor>> = vec![];

        for name in &self.proxy_names {
            match name.as_str() {
                "sparkle" => {
                    proxies.push(DynComponent::new(sparkle::SparkleComponent::new()));
                }
                "ferris" => {
                    // Enable all Ferris tools by default
                    let ferris_config = symposium_ferris::Ferris::new()
                        .crate_sources(true)
                        .rust_researcher(true);
                    proxies.push(DynComponent::new(symposium_ferris::FerrisComponent::new(
                        ferris_config,
                    )));
                }
                "cargo" => {
                    proxies.push(DynComponent::new(symposium_cargo::CargoProxy));
                }
                other => {
                    tracing::warn!("Unknown proxy name: {}", other);
                }
            }
        }

        proxies
    }

    /// Configure a conductor with tracing and other settings.
    fn configure_conductor<L: sacp_conductor::ConductorLink>(
        &self,
        conductor: Conductor<L>,
    ) -> Result<Conductor<L>, sacp::Error> {
        let Some(ref dir) = self.trace_dir else {
            return Ok(conductor);
        };

        std::fs::create_dir_all(dir).map_err(sacp::Error::into_internal_error)?;
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let trace_path = dir.join(format!("{}.jsons", timestamp));
        let conductor = conductor
            .trace_to_path(&trace_path)
            .map_err(sacp::Error::into_internal_error)?;
        tracing::info!("Tracing to {}", trace_path.display());

        Ok(conductor)
    }
}

impl Default for SymposiumConfig {
    fn default() -> Self {
        Self::new()
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
    /// Create a new Symposium from configuration.
    pub fn new(config: SymposiumConfig) -> Self {
        Symposium { config }
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

        tracing::debug!("Creating conductor (proxy mode)");
        let conductor = Conductor::new_proxy(
            "symposium",
            {
                let config = config.clone();
                async move |init_req| {
                    tracing::info!(
                        "Building proxy chain with extensions: {:?}",
                        config.proxy_names
                    );
                    let proxies = config.build_proxies();
                    Ok((init_req, proxies))
                }
            },
            McpBridgeMode::default(),
        );

        let conductor = config.configure_conductor(conductor)?;

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

        tracing::debug!("Creating conductor (agent mode)");
        let conductor = Conductor::new_agent(
            "symposium",
            {
                let config = config.clone();
                async move |init_req| {
                    tracing::info!(
                        "Building proxy chain with extensions: {:?}",
                        config.proxy_names
                    );
                    let proxies = config.build_proxies();
                    Ok((init_req, proxies, agent))
                }
            },
            McpBridgeMode::default(),
        );

        let conductor = config.configure_conductor(conductor)?;

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}
