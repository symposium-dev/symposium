# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/symposium.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines two config types: user-wide `Config` (stored at `~/.symposium/config.toml`) with `AgentConfig`, logging, plugin sources, and hook settings; and `ProjectConfig` (stored at `.symposium/config.toml`) with optional agent override, skills, and workflows. Provides `resolve_agent_name()` and `resolve_sync_default()` for merging project settings over user settings.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization commands

Implements `symposium init`. Three entry points: `init_user()` prompts for agent and writes user config; `init_project()` finds the workspace root, creates project config, and runs sync; `init_default()` does both as needed.

### `sync.rs` — synchronization commands

Implements `symposium sync`. Two main flows: `sync_workspace()` scans workspace dependencies, matches against plugin skill predicates, and merges into `.symposium/config.toml`; `sync_agent()` reads the project config and installs enabled skills into agent-specific directories while registering hooks.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Each plugin contains `SkillGroup`s (which crates, where to find the skills) and `Hook`s (event handlers). Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module does the actual work: resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, evaluates crate predicates, and formats output. Separates results into `always` (inlined) vs `optional` (listed with metadata).

### `hook.rs` and `session_state.rs` — hook handling

`hook.rs` handles the three hook events: `PostToolUse` (tracks which skills the agent has loaded and reminds the agent to run `cargo fmt` when Rust files change), `UserPromptSubmit` (scans prompts for crate mentions and nudges about unloaded skills), and `PreToolUse` (dispatches to plugin-defined hook commands). `session_state.rs` persists per-session data (activations, nudge history, prompt count, Rust file snapshots) as JSON files.

### `cargo_fmt.rs` — cargo fmt reminder

Detects changes to `*.rs` files by comparing modification times against a snapshot stored in session state. When a change is detected, injects a suggestion into the agent's context to run `cargo fmt`. The reminder frequency is configurable via `fmt-reminder` under `[hooks]` in `config.toml`.

### `dispatch.rs` — shared CLI/MCP dispatch

The convergence point for the legacy CLI and MCP. Defines `SharedCommand` (clap-derived enum) and routes `start` and `crate` commands to the right handler. Both the legacy CLI (`main.rs`) and `mcp.rs` (MCP server) call into this layer.
