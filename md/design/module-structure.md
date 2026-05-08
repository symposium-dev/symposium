# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/cargo-agents.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines the user-wide `Config` (stored at `~/.symposium/config.toml`) with `[[agent]]` entries, logging, plugin sources, and defaults. Provides `plugin_sources()` to resolve the effective list of plugin source directories.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization command

Implements `cargo agents init`. Prompts for agents (or accepts `--add-agent`/`--remove-agent` flags), writes user config, and registers global hooks.

### `sync.rs` — synchronization command

Implements `cargo agents sync`. Scans workspace dependencies, finds applicable skills from plugin sources, installs them into each configured agent's skill directory, manages a per-agent `.symposium.toml` manifest to track installed skills, and cleans up stale skills. Also provides `register_hooks()` for use by `init`.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Validation here turns the raw TOML into:
- `Installation` entries (optional `source`, optional `executable`/`script`, optional `args`, plus `requirements` and `install_commands`) collected on `Plugin.installations`. Inline installation references on hooks or other installations are *promoted* into synthetic `Installation` entries with derived names (`<hook>` for an inline `command`, `<owner>__req_<i>` for an inline requirement), so all references in the validated form are plain names.
- `Hook` entries with `command: String` (the name of an `Installation`) plus optional hook-level `executable` / `script` / `args`. Validation guarantees at most one of `executable`/`script` is set across hook + installation, and at most one layer sets `args`.

Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `installation.rs` — sources and acquisition

Defines `Source` (the `source = "..."`-tagged enum: `cargo`, `github`, `binary`) and `acquire_source`, which downloads / installs / clones the source and returns an `AcquiredSource` whose `resolve_executable` / `resolve_script` methods turn a relative `executable`/`script` name into a concrete path. The `Runnable` enum (`Exec(PathBuf)` or `Script(PathBuf)`) is the final form a hook command resolves to. The `git` submodule handles GitHub tarball acquisition and caching.

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, and evaluates crate predicates at each level (plugin, group, skill) to determine which skills apply.

### `hook.rs` — hook handling

Handles the hook pipeline: parse agent wire-format input → auto-sync → builtin dispatch → plugin hook dispatch → serialize output. The dispatch path matches plugin `Hook`s against the event, builds a `ResolvedHook` per match (looking up the named installations on the plugin), then for each `ResolvedHook`: acquires its `requirements` (best-effort), runs `install_commands` after the source step, picks a `Runnable` from (hook-or-install) `executable`/`script`, and spawns it (binary directly for `Exec`, via `sh <path>` for `Script`). Format routing converts hook output between agent wire formats and the symposium canonical format.

### `crate_command.rs` — crate source lookup

Contains `dispatch_crate()`, which resolves a crate's version and fetches its source code. Called by the CLI's `crate-info` command.
