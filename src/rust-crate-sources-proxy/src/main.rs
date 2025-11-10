//! Rust Crate Sources Proxy - Main entry point

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    rust_crate_sources_proxy::run().await
}
