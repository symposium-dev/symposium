use anyhow::Result;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::role;
use sacp::{ByteStreams, ConnectTo, RunWithConnectionTo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::crate_sources;
use crate::skills;

pub async fn serve() -> Result<()> {
    let server = build_server();
    let stdio = ByteStreams::new(
        tokio::io::stdout().compat_write(),
        tokio::io::stdin().compat(),
    );
    server.connect_to(stdio).await?;
    Ok(())
}

fn build_server() -> McpServer<role::mcp::Client, impl RunWithConnectionTo<role::mcp::Client>> {
    McpServer::builder("symposium".to_string())
        .instructions(
            "Symposium — AI the Rust Way. \
             Use the `rust` tool for Rust development guidance. \
             Use the `crate` tool to find crate sources and guidance.",
        )
        .tool_fn(
            "rust",
            RUST_TOOL_DESCRIPTION,
            async move |input: RustToolInput, _cx: McpConnectionTo<role::mcp::Client>| {
                let output = execute_rust_command(&input.command);
                Ok(RustToolOutput { output })
            },
            sacp::tool_fn!(),
        )
        .tool_fn(
            "crate",
            CRATE_TOOL_DESCRIPTION,
            async move |input: CrateToolInput, _cx: McpConnectionTo<role::mcp::Client>| {
                let cwd = std::env::current_dir()
                    .map_err(|e| sacp::util::internal_error(format!("failed to get cwd: {e}")))?;

                let workspace = crate_sources::workspace_semver_pairs(&cwd);

                let registry = crate::plugins::load_registry();

                let output = match input {
                    CrateToolInput::List => skills::list_output(&registry, &workspace).await,
                    CrateToolInput::Info { name, version } => {
                        skills::info_output(&name, version.as_deref(), &registry, &workspace)
                            .await
                            .map_err(|e| sacp::util::internal_error(format!("{e}")))?
                    }
                };

                Ok(CrateToolOutput { output })
            },
            sacp::tool_fn!(),
        )
        .build()
}

// --- Rust tool ---

const RUST_TOOL_DESCRIPTION: &str = "\
Use the Symposium Rust tool for guidance on Rust best practices \
and how to use dependencies of the current project. \
Execute the tool with the argument `help` to learn more.";

#[derive(Deserialize, JsonSchema)]
struct RustToolInput {
    /// The command to run (e.g., "help")
    command: String,
}

#[derive(Serialize, JsonSchema)]
struct RustToolOutput {
    output: String,
}

pub fn execute_rust_command(command: &str) -> String {
    let command = command.trim();

    if command == "help" {
        return crate::tutorial::render_mcp();
    }

    format!("Unknown command: {command}. Use `help` to see available commands.")
}

// --- Crate tool ---

const CRATE_TOOL_DESCRIPTION: &str = "\
Find Rust crate source code and guidance. \
Use this to inspect crate implementations, understand APIs, or debug issues.\n\n\
Pass a `List` command to see crates where specialized guidance is available.\n\
Pass an `Info` command with a crate name to locate its source code.\n\n\
If no version is given, defaults to the version used in the current workspace, \
or the latest version on crates.io if the crate is not a dependency.";

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "command")]
enum CrateToolInput {
    /// List crates where specialized guidance is available
    List,
    /// Get info and source location for a specific crate
    Info {
        /// Crate name (e.g., "serde", "tokio")
        name: String,
        /// Optional version constraint (e.g., "1.0.3", "^1.0")
        #[serde(default)]
        version: Option<String>,
    },
}

#[derive(Serialize, JsonSchema)]
struct CrateToolOutput {
    output: String,
}
