# The `symposium` command

`symposium` is a cargo subcommand that manages agent extensions for Rust projects. It discovers skills, workflows, and MCP servers based on your project's dependencies and configures your AI agent to use them.

## Subcommands

| Command | Description |
|---------|-------------|
| [`symposium init`](./symposium-init.md) | Set up user-wide or project-level configuration |
| [`symposium sync`](./symposium-sync.md) | Synchronize configuration with workspace dependencies and agent |
| [`symposium hook`](./symposium-hook.md) | Hook entry point invoked by your agent (internal) |

## Global options

| Flag | Description |
|------|-------------|
| `--help` | Print help |
| `--version` | Print version |
