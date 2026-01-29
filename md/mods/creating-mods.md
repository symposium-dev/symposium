# Creating Agent Mods

A Symposium agent mod is an [ACP (Agent Client Protocol)](https://agentclientprotocol.com/) proxy that sits between the client and the agent. Proxies can intercept and transform messages, inject context, provide MCP tools, and coordinate agent behavior. Agent mods are typically distributed as Rust crates on crates.io.

## Basic Structure

Your mod crate should:

1. Implement an ACP proxy using the `sacp` crate
2. Produce a binary that speaks ACP over stdio
3. Include Symposium metadata in Cargo.toml

See the [sacp cookbook on building proxies](https://docs.rs/sacp-cookbook/latest/sacp_cookbook/#building-proxies) for implementation details and examples.

## Cargo.toml Metadata

Add metadata to tell Symposium how to run your mod:

```toml
[package]
name = "my-mod"
version = "0.1.0"
description = "Help agents work with MyLibrary"

[package.metadata.symposium]
# Optional: specify which binary if your crate has multiple
binary = "my-mod"

# Optional: arguments to pass when spawning
args = []

# Optional: environment variables
env = { MY_CONFIG = "value" }
```

The `name`, `description`, and `version` come from the standard `[package]` section.

## Testing Your Mod

Before publishing:

1. Install locally: `cargo install --path .`
2. Test with Symposium: add to your local config and verify it loads correctly
3. Check ACP compliance: ensure your proxy handles `proxy/initialize` correctly
