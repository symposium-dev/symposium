# Creating Extensions

A Symposium extension is an MCP (Model Context Protocol) server that provides tools, prompts, or resources to AI agents. Extensions are typically distributed as Rust crates on crates.io.

## Basic Structure

Your extension crate should:

1. Implement an MCP server (see [MCP specification](https://modelcontextprotocol.io/))
2. Produce a binary that speaks MCP over stdio
3. Include Symposium metadata in Cargo.toml

## Cargo.toml Metadata

Add metadata to tell Symposium how to run your extension:

```toml
[package]
name = "my-extension"
version = "0.1.0"
description = "Help agents work with MyLibrary"

[package.metadata.symposium]
# Optional: specify which binary if your crate has multiple
binary = "my-extension"

# Optional: arguments to pass when spawning
args = ["--mcp"]

# Optional: environment variables
env = { MY_CONFIG = "value" }
```

The `name`, `description`, and `version` come from the standard `[package]` section.

## Example

A minimal extension that provides a single tool:

```rust
use mcp_server::{Server, Tool, ToolResult};

#[tokio::main]
async fn main() {
    let server = Server::new("my-extension")
        .tool("greet", "Say hello", |name: String| async move {
            ToolResult::text(format!("Hello, {}!", name))
        });
    
    server.run_stdio().await;
}
```

## Testing Your Extension

Before publishing:

1. Install locally: `cargo install --path .`
2. Test with Symposium: add to your local config and verify tools appear
3. Check MCP compliance: ensure your server handles initialization correctly
