# Implementation overview

Symposium is a single Rust binary crate. The source is in `src/`:

| File | Purpose |
|------|---------|
| `main.rs` | CLI entry point using clap. Defines subcommands: `tutorial`, `mcp`, `hook`, `crate`, `update`. Initializes config, logging, and plugin source updates at startup. |
| `config.rs` | Reads `~/.symposium/config.toml`, caches the result in a thread-local, and initializes tracing with a file appender to `~/.symposium/logs/`. |
| `hook.rs` | Handles hook events. Reads the event JSON from stdin, matches hooks from loaded plugins, and spawns hook commands. |
| `tutorial.rs` | Renders the tutorial template (`md/tutorial.md`). |
| `mcp.rs` | MCP server over stdio using `sacp`. Exposes `rust` and `crate` tools. |
| `crate_sources/` | Crate source fetching: version resolution, cache lookup, download+extraction. |
| `plugins.rs` | Plugin registry: loads TOML manifests from configured plugin sources, produces `Vec<Plugin>` as a table of contents. Defines `SkillGroup`, `PluginSource`, `Hook` types. Does not load skill content — that is handled by the skills layer. |
| `git_source.rs` | GitHub URL parsing, API client, and plugin cache manager. Downloads tarballs, extracts subdirectories, caches under `~/.symposium/cache/` with commit SHA freshness checking. Used by both plugin source fetching and skill source fetching. |
| `skills.rs` | Skill model, frontmatter parsing, discovery, and crate advice output. Given loaded plugins, resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, evaluates `crates` predicates, and formats output. Skills follow the [agentskills.io](https://agentskills.io/specification.md) format. Shared `list_output()` and `info_output()` helpers used by both CLI and MCP. |
| `predicate.rs` | Parser and evaluator for crate predicates. Supports crate atoms (`serde`, `tokio>=1.0`) with optional version constraints. |

## Key dependencies

- **sacp / sacp-tokio** — MCP server implementation
- **clap** — CLI argument parsing
- **tracing / tracing-subscriber / tracing-appender** — Structured logging to `~/.symposium/logs/`
- **toml** — Config file parsing
- **dirs** — Home directory resolution
- **cargo_metadata** — Workspace dependency resolution
- **reqwest** — HTTP client for downloading crates
- **flate2 / tar** — Crate archive extraction
- **crates_io_api** — Crates.io version lookup
- **semver** — Version constraint parsing
- **expect-test** — Snapshot testing (dev dependency)

## Build and test

```bash
cargo check
cargo test
cargo run -- tutorial      # print the tutorial
cargo run -- hook pre-tool-use  # reads event JSON from stdin
cargo run -- crate tokio   # find crate source location
cargo run -- crate --list  # list skills available for workspace crates
cargo run -- update        # refresh plugin sources
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
