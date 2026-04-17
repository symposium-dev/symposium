# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/symposium.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines the user-wide `Config` (stored at `~/.symposium/config.toml`) with `[[agent]]` entries, logging, plugin sources, and defaults. Provides `plugin_sources()` to resolve the effective list of plugin source directories.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization command

Implements `symposium init`. Prompts for agents (or accepts `--add-agent`/`--remove-agent` flags), writes user config, and registers global hooks.

### `sync.rs` — synchronization command

Implements `symposium sync`. Scans workspace dependencies, finds applicable skills from plugin sources, installs them into each configured agent's skill directory, manages a per-agent `.symposium.toml` manifest to track installed skills, and cleans up stale skills. Also provides `register_hooks()` for use by `init`.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Each plugin contains `SkillGroup`s (which crates, where to find the skills) and `Hook`s (event handlers). Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module does the actual work: resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, evaluates crate predicates, and formats output. Separates results into `always` (inlined) vs `optional` (listed with metadata).

### `hook.rs` and `session_state.rs` — hook handling

`hook.rs` handles the three hook events: `PostToolUse` (tracks which skills the agent has loaded), `UserPromptSubmit` (scans prompts for crate mentions and nudges about unloaded skills), and `PreToolUse` (dispatches to plugin-defined hook commands). When `auto-sync` is enabled, runs `sync` as a side effect during hook invocations. `session_state.rs` persists per-session data (activations, nudge history, prompt count) as JSON files.

### `dispatch.rs` — shared dispatch logic

Contains `dispatch_crate()`, which resolves crate skills and formats output. Called by the CLI's `crate` command and the test harness's `invoke()` helper.
