use clap::Parser;
use conductor::ConductorArgs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ConductorArgs::parse().run().await
}
