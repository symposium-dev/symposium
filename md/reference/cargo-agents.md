# The `symposium` command

`symposium` manages agent extensions for Rust projects. It discovers skills based on your project's dependencies and configures your AI agent to use them.

## Subcommands

| Command | Description |
|---------|-------------|
| [`symposium init`](./cargo-agents-init.md) | Set up user-wide configuration |
| [`symposium sync`](./cargo-agents-sync.md) | Synchronize skills with workspace dependencies |
| [`symposium start`](./cargo-agents-start.md) | Get Rust guidance and list available crate skills |
| [`symposium crate`](./cargo-agents-crate.md) | Find crate sources and guidance |
| [`symposium plugin`](./cargo-agents-plugin.md) | Manage plugin sources |
| [`symposium hook`](./cargo-agents-hook.md) | Hook entry point invoked by your agent (internal) |
| [`symposium mcp`](./cargo-agents-mcp.md) | Run as an MCP server (stdio transport) |

## Global options

| Flag | Description |
|------|-------------|
| `--update <LEVEL>` | Plugin source update behavior: `none` (default), `check`, `fetch` |
| `-q`, `--quiet` | Suppress status output |
| `--help` | Print help |
| `--version` | Print version |
