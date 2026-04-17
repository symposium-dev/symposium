use anyhow::Result;
use clap::{Parser, Subcommand};
use indoc::indoc;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::role;
use sacp::{ByteStreams, ConnectTo, RunWithConnectionTo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::config::Symposium;
use crate::dispatch::{self, DispatchResult};

#[derive(Debug, Parser)]
#[command(name = "symposium", no_binary_name = true, about = "")]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum McpCommand {
    /// Find crate sources and guidance
    Crate {
        /// Crate name (omit to use --list)
        name: Option<String>,

        /// Version constraint (e.g., "1.0.3", "^1.0"); defaults to workspace version or latest
        #[arg(long)]
        version: Option<String>,

        /// List all workspace dependency crates
        #[arg(long)]
        list: bool,
    },
}

pub async fn serve(sym: &Symposium, cwd: &std::path::Path) -> Result<()> {
    let server = build_server(sym.clone(), cwd.to_path_buf());
    let stdio = ByteStreams::new(
        tokio::io::stdout().compat_write(),
        tokio::io::stdin().compat(),
    );
    server.connect_to(stdio).await?;
    Ok(())
}

fn build_server(
    sym: Symposium,
    cwd: std::path::PathBuf,
) -> McpServer<role::mcp::Client, impl RunWithConnectionTo<role::mcp::Client>> {
    McpServer::builder("symposium".to_string())
        .instructions("Symposium — tools for agentic Rust development")
        .tool_fn(
            "rust",
            RUST_TOOL_DESCRIPTION,
            async move |input: RustToolInput, _cx: McpConnectionTo<role::mcp::Client>| {
                let parsed = McpArgs::try_parse_from(&input.args)
                    .map_err(|e| sacp::util::internal_error(format!("invalid arguments: {e}")))?;

                let McpCommand::Crate {
                    name,
                    version,
                    list,
                } = parsed.command;

                let result = dispatch::dispatch_crate(
                    &sym,
                    name.as_deref(),
                    version.as_deref(),
                    list,
                    &cwd,
                )
                .await;

                match result {
                    DispatchResult::Ok(output) => Ok(RustToolOutput { output }),
                    DispatchResult::Err(e) => Err(sacp::util::internal_error(e)),
                }
            },
            sacp::tool_fn!(),
        )
        .build()
}

const RUST_TOOL_DESCRIPTION: &str = indoc! {"
    Critical. Invoke this tool with [\"crate\", \"--list\"] before authoring Rust code.

    Commands:
      [\"crate\", \"--list\"] — list workspace crates with skills
      [\"crate\", \"<name>\"] — get crate guidance
      [\"help\"]           — show help
"};

#[derive(Deserialize, JsonSchema)]
struct RustToolInput {
    /// Command arguments (e.g., ["crate", "--list"], ["crate", "tokio"])
    args: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct RustToolOutput {
    output: String,
}
