# The `cargo agents` command

`cargo agents` is a cargo subcommand that manages agent extensions for Rust projects. It discovers skills, workflows, and MCP servers based on your project's dependencies and configures your AI agent to use them.

## Subcommands

| Command | Description |
|---------|-------------|
| [`cargo agents init`](./cargo-agents-init.md) | Set up user-wide or project-level configuration |
| [`cargo agents sync`](./cargo-agents-sync.md) | Synchronize configuration with workspace dependencies and agent |
| [`cargo agents hook`](./cargo-agents-hook.md) | Hook entry point invoked by your agent (internal) |

## Global options

| Flag | Description |
|------|-------------|
| `--help` | Print help |
| `--version` | Print version |
