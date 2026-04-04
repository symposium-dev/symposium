use std::sync::Arc;

use anyhow::Result;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::role;
use sacp::{ByteStreams, ConnectTo, RunWithConnectionTo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::config::Symposium;
use crate::dispatch::{self, DispatchResult};

pub async fn serve(sym: Arc<Symposium>) -> Result<()> {
    let server = build_server(sym);
    let stdio = ByteStreams::new(
        tokio::io::stdout().compat_write(),
        tokio::io::stdin().compat(),
    );
    server.connect_to(stdio).await?;
    Ok(())
}

fn build_server(
    sym: Arc<Symposium>,
) -> McpServer<role::mcp::Client, impl RunWithConnectionTo<role::mcp::Client>> {
    McpServer::builder("symposium".to_string())
        .instructions(
            "Symposium — AI the Rust Way. \
             Critical: invoke the `rust` tool with `[\"start\"]` before authoring Rust code.",
        )
        .tool_fn(
            "rust",
            RUST_TOOL_DESCRIPTION,
            async move |input: RustToolInput, _cx: McpConnectionTo<role::mcp::Client>| {
                let cwd = std::env::current_dir()
                    .map_err(|e| sacp::util::internal_error(format!("failed to get cwd: {e}")))?;

                let result = dispatch::dispatch(&sym, &input.args, &cwd).await;

                match result {
                    DispatchResult::Ok(output) => Ok(RustToolOutput { output }),
                    DispatchResult::Err(e) => Err(sacp::util::internal_error(e)),
                }
            },
            sacp::tool_fn!(),
        )
        .build()
}

const RUST_TOOL_DESCRIPTION: &str = "\
Critical. Invoke this tool with [\"start\"] as argument before authoring or working with Rust code.\n\n\
Commands:\n\
  [\"start\"] — Rust guidance + available crate skills for this workspace\n\
  [\"crate\", \"--list\"] — list workspace crates with available skills\n\
  [\"crate\", \"<name>\"] — get crate info and guidance\n\
  [\"crate\", \"<name>\", \"--version\", \"<ver>\"] — specific version\n\
  [\"help\"] — show help";

#[derive(Deserialize, JsonSchema)]
struct RustToolInput {
    /// Command arguments (e.g., ["start"], ["crate", "tokio"])
    args: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct RustToolOutput {
    output: String,
}
