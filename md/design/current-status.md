# Current status

Symposium is in early development. This page describes what works today. For the full vision, see the [design overview](./overview.md).

## What works

### Hooks

`symposium hook <event>` handles hook events from editor plugins. The Claude Code plugin registers a `PreToolUse` hook that invokes this subcommand, passing event data via stdin. Currently logs hook events to `~/.symposium/logs/`.

### Configuration

`~/.symposium/config.toml` provides user configuration. Currently supports:

```toml
[logging]
level = "info"  # trace, debug, info, warn, error
```

### Logging

All symposium invocations emit structured logs to `~/.symposium/logs/symposium.log`. The log level is configured via `config.toml`.

### MCP server

`symposium mcp` runs an MCP server over stdio, exposing a `rust` tool. The tutorial is installed as the server's instructions.

### Tutorial

`symposium tutorial` prints a guide for agents (and humans) on how to use Symposium.

## How to use it

There are three ways to use Symposium today:

### Claude Code plugin

Install the plugin to get a `/symposium:rust` skill and automatic `PreToolUse` hook integration. The plugin includes a bootstrap script that finds or downloads the binary automatically.

```bash
claude --plugin-dir path/to/agent-plugins/claude-code
```

See [How to install](../install.md) for details.

### MCP server

Configure your editor or agent to run `symposium mcp` as an MCP server over stdio.

### Direct CLI

If Symposium is on your PATH:

```bash
symposium tutorial
symposium hook pre-tool-use  # reads event JSON from stdin
```

## What's not yet implemented

The [design overview](./overview.md) describes the full architecture. The following are planned but not yet built:

- **Token-optimized cargo** — Cargo output filtering for token efficiency (temporarily removed, returning in a future release)
- **Plugin system** — `symposium.toml`-based plugins providing skills, hooks, and other capabilities
- **Per-crate skills** — Guidance documents tailored to specific dependencies
- **ACP agent** — Full interception via the Agent Client Protocol
- **Editor extensions** — Native integrations for VSCode, Zed, and IntelliJ
- **`symposium skill`** — CLI for listing and retrieving skills
- **`symposium update`** — Self-update mechanism
- **Plugin repository** — Central repository of community plugins
