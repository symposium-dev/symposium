# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and binary (`src/main.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Each plugin contains `SkillGroup`s (which crates, where to find the skills) and `Hook`s (event handlers). Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module does the actual work: resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, evaluates crate predicates, and formats output. Separates results into `always` (inlined) vs `optional` (listed with metadata).

### `hook.rs` and `session_state.rs` — hook handling

`hook.rs` handles the three hook events: `PostToolUse` (tracks which skills the agent has loaded), `UserPromptSubmit` (scans prompts for crate mentions and nudges about unloaded skills), and `PreToolUse` (dispatches to plugin-defined hook commands). `session_state.rs` persists per-session data (activations, nudge history, prompt count) as JSON files.

### `dispatch.rs` — shared CLI/MCP dispatch

The convergence point for CLI and MCP. Defines `SharedCommand` (clap-derived enum) and routes `start` and `crate` commands to the right handler. Both `main.rs` (CLI) and `mcp.rs` (MCP server) call into this layer.
