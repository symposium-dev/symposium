use crate::conductor::Conductor;

mod component;
mod conductor;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct ConductorArgs {
    /// List of proxy commands to chain together
    pub proxies: Vec<String>,
}

impl ConductorArgs {
    pub async fn run(self) -> anyhow::Result<()> {
        Conductor::run(self.proxies).await
    }
}
