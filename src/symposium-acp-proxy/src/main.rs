//! Symposium ACP Proxy - Main entry point

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-proxy")]
#[command(about = "Symposium ACP proxy - orchestrates dynamic component chains between editor and agent")]
struct Cli {
    #[command(flatten)]
    args: symposium_acp_proxy::SymposiumArgs,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    symposium_acp_proxy::run(&cli.args).await
}
