# Implementation overview

Symposium is a single Rust binary crate. The source is in `src/` with five files:

| File | Purpose |
|------|---------|
| `main.rs` | CLI entry point using clap. Defines three subcommands: `tutorial`, `mcp`, `hook`. Initializes config and logging at startup. |
| `config.rs` | Reads `~/.symposium/config.toml`, caches the result in a thread-local, and initializes tracing with a file appender to `~/.symposium/logs/`. |
| `hook.rs` | Handles hook events. Reads the event JSON from stdin and logs it. |
| `tutorial.rs` | Renders the tutorial template (`md/tutorial.md`). |
| `mcp.rs` | MCP server over stdio using `sacp`. Exposes a single `rust` tool that returns the tutorial. |

## Key dependencies

- **sacp / sacp-tokio** — MCP server implementation
- **clap** — CLI argument parsing
- **tracing / tracing-subscriber / tracing-appender** — Structured logging to `~/.symposium/logs/`
- **toml** — Config file parsing
- **dirs** — Home directory resolution

## Build and test

```bash
cargo check
cargo test
cargo run -- tutorial      # print the tutorial
cargo run -- hook pre-tool-use  # reads event JSON from stdin
```

## Agent plugin generation

The Claude Code plugin skill is generated from a template:

```bash
just skill
```

This runs `cargo run -- tutorial`, appends the output to `agent-plugins/claude-code/skills/rust/SKILL.md.tmpl`, and writes the result to `SKILL.md`.

## Claude Code plugin structure

The plugin at `agent-plugins/claude-code/` contains:

- `.claude-plugin/plugin.json` — Plugin manifest
- `scripts/symposium.sh` — Bootstrap script shared by skills and hooks
- `skills/rust/SKILL.md` — Generated skill document
- `hooks/hooks.json` — Hook configuration (registers `PreToolUse` hook)
