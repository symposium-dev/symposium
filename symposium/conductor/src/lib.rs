use crate::{component::CommandComponentProvider, conductor::Conductor};

pub mod component;
pub mod conductor;
mod mcp_bridge;

#[cfg(test)]
mod conductor_tests;

#[cfg(test)]
mod mcp_bridge_rmcp_test;

#[cfg(test)]
mod test_util;

use clap::{Parser, Subcommand};
use component::ComponentProvider;
use tokio::io::{stdin, stdout};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct ConductorArgs {
    #[command(subcommand)]
    pub command: ConductorCommand,
}

#[derive(Subcommand, Debug)]
pub enum ConductorCommand {
    /// Run as agent orchestrator managing a proxy chain
    Agent {
        /// List of proxy commands to chain together
        proxies: Vec<String>,
    },
    /// Run as MCP bridge connecting stdio to TCP
    Mcp {
        /// TCP port to connect to on localhost
        port: u16,
    },
}

impl ConductorArgs {
    pub async fn run(self) -> Result<(), agent_client_protocol::Error> {
        match self.command {
            ConductorCommand::Agent { proxies } => {
                let providers = proxies
                    .into_iter()
                    .map(|s| CommandComponentProvider::new(s))
                    .collect();

                Conductor::run(stdout().compat_write(), stdin().compat(), providers).await
            }
            ConductorCommand::Mcp { port } => mcp_bridge::run_mcp_bridge(port).await,
        }
    }
}
