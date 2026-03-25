use anyhow::Result;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::role;
use sacp::{ByteStreams, ConnectTo, RunWithConnectionTo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const TOOL_DESCRIPTION: &str = "\
Use the Symposium Rust tool to run cargo, learn about Rust best practices, \
and learn how to use dependencies of the current project. \
Execute the tool with the argument `help` to learn more.";

#[derive(Deserialize, JsonSchema)]
struct RustToolInput {
    /// The command to run (e.g., "help", "cargo check", "cargo test --all")
    command: String,
}

#[derive(Serialize, JsonSchema)]
struct RustToolOutput {
    output: String,
}

fn build_server() -> McpServer<role::mcp::Client, impl RunWithConnectionTo<role::mcp::Client>> {
    McpServer::builder("symposium".to_string())
        .instructions(
            "Symposium — AI the Rust Way. Use the `rust` tool for all Rust development tasks.",
        )
        .tool_fn(
            "rust",
            TOOL_DESCRIPTION,
            async move |input: RustToolInput, _cx: McpConnectionTo<role::mcp::Client>| {
                let output = execute_command(&input.command).await;
                Ok(RustToolOutput { output })
            },
            sacp::tool_fn!(),
        )
        .build()
}

async fn execute_command(command: &str) -> String {
    let command = command.trim();

    if command == "help" {
        return crate::tutorial::render_mcp();
    }

    let args = match shell_words::split(command) {
        Ok(args) => args,
        Err(e) => return format!("Error parsing command: {e}"),
    };

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return format!("Error finding symposium binary: {e}"),
    };

    let output = match tokio::process::Command::new(exe).args(&args).output().await {
        Ok(output) => output,
        Err(e) => return format!("Error running command: {e}"),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = stdout.into_owned();
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }

    if result.is_empty() {
        if output.status.success() {
            "Command completed successfully.".to_string()
        } else {
            format!("Command failed with exit code: {}", output.status)
        }
    } else {
        result
    }
}

pub async fn serve() -> Result<()> {
    let server = build_server();
    let stdio = ByteStreams::new(
        tokio::io::stdout().compat_write(),
        tokio::io::stdin().compat(),
    );
    server.connect_to(stdio).await?;
    Ok(())
}
