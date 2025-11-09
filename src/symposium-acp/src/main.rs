//! Symposium ACP - Main entry point

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    symposium_acp::run().await
}
