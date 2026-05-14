# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/cargo-agents.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines the user-wide `Config` (stored at `~/.symposium/config.toml`) with `[[agent]]` entries, logging, plugin sources, defaults, and `auto-update` (off/warn/on, default warn). Provides `plugin_sources()` to resolve the effective list of plugin source directories.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization command

Implements `cargo agents init`. Prompts for agents (or accepts `--add-agent`/`--remove-agent` flags), writes user config, and registers global hooks.

### `sync.rs` — synchronization command

Implements `cargo agents sync`. Scans workspace dependencies, finds applicable skills from plugin sources, installs them into each configured agent's skill directory, and drops a `.symposium` marker file into each installed skill directory. On subsequent syncs, scans every agent's skills parent directory and reaps any marker-bearing subdirectory it didn't install this time, leaving user-managed skills (which lack the marker) untouched. Writes a `.gitignore` with `*` into every directory it creates. Also provides `register_hooks()` for use by `init`.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Validation here turns the raw TOML into:
- `Installation` entries (optional `source`, optional `executable`/`script`, optional `args`, plus `requirements` and `install_commands`) collected on `Plugin.installations`. Inline installation references on hooks or other installations are *promoted* into synthetic `Installation` entries with derived names (`<hook>` for an inline `command`, `<owner>__req_<i>` for an inline requirement), so all references in the validated form are plain names.
- `Hook` entries with `command: String` (the name of an `Installation`) plus optional hook-level `executable` / `script` / `args`. Validation guarantees at most one of `executable`/`script` is set across hook + installation, and at most one layer sets `args`.

Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `installation.rs` — sources and acquisition

Defines `Source` (the `source = "..."`-tagged enum: `cargo`, `github`, `binary`) and `acquire_source`, which downloads / installs / clones the source and returns an `AcquiredSource` whose `resolve_executable` / `resolve_script` methods turn a relative `executable`/`script` name into a concrete path. The `Runnable` enum (`Exec(PathBuf)` or `Script(PathBuf)`) is the final form a hook command resolves to. The `git` submodule handles GitHub tarball acquisition and caching.

Validates skill group source constraints at parse time: mutual exclusivity of `source.path`/`source.git`/`source.crate_path`, and the requirement that `source.crate_path` has at least one non-wildcard predicate.

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, and evaluates crate predicates at each level (plugin, group, skill) to determine which skills apply. For `source.crate_path` groups, resolves predicates to a matched crate set and fetches each crate's source via `RustCrateFetch`.

Each applicable skill carries a `SkillOrigin` describing *where its bytes live*, used at sync time for dedup and install-path disambiguation. What matters for identity is the on-disk location of the skill, not which plugin manifest pointed at it — two plugins in the same source pointing at the same skill bundle dedupe.
- `Crate { name, version }` — from a crate-source resolution (`source = "crate"` / `source.crate_path`). Two `Crate` origins with the same `(name, version)` are the same logical skill, regardless of which plugin pointed at them.
- `Git { repo, commit_sha, skill_path }` — from a `source.git` group. Identity is the triple `(owner/repo, resolved commit SHA, SKILL.md path within the repo tree)`. Two plugins that pointed at the same repo via different URL forms (root URL vs. `tree/<ref>/<subpath>`) collapse to one install when they end up loading the same SKILL.md from the same commit; different SKILL.md files within one repo stay distinct.
- `Source { source_name, skill_path }` — from a plugin's `source.path` group, or from a standalone `SKILL.md` discovered in a registry source. `source_name` is the registry source's display name (e.g. `"user-plugins"`); `skill_path` is the SKILL.md's parent directory relative to the source root (canonicalized first, so `../`-laden joins collapse to the same string as a direct standalone walk).

`sync` prefers the plain `<agent-skills-dir>/<skill-name>/` and only falls back to `<skill-name>-<origin-disambiguator>/` when needed: when more than one origin claims the same skill name, or when the unsuffixed slot is already occupied by a user-managed directory (one without the `.symposium` marker). The disambiguator is human-readable for `Crate` (`<crate-name>-<version>`) and an 8-hex SHA-256 prefix for the other variants. The `.symposium` marker, wildcard `.gitignore`, and stale-cleanup walk all key on the marker file rather than directory name shape, so transitions between unsuffixed and suffixed names self-heal across syncs.

### `hook.rs` — hook handling

Handles the hook pipeline: parse agent wire-format input → auto-sync → builtin dispatch → plugin hook dispatch → serialize output. The dispatch path matches plugin `Hook`s against the event, builds a `ResolvedHook` per match (looking up the named installations on the plugin), then for each `ResolvedHook`: acquires its `requirements` (best-effort), runs `install_commands` after the source step, picks a `Runnable` from (hook-or-install) `executable`/`script`, and spawns it (binary directly for `Exec`, via `sh <path>` for `Script`). Format routing converts hook output between agent wire formats and the symposium canonical format.

### `state.rs` — persistent state

Manages `state.toml` in the config directory. Tracks the semver of the binary that last touched the directory (for future migration hooks) and the timestamp of the last update check (to throttle crates.io queries to once per 24 hours). `ensure_current()` is called on startup to silently stamp the current version. `should_check_for_update()` / `record_update_check()` gate the auto-update flow.

### `self_update.rs` — self-update

Implements `cargo agents self-update`. Queries the registry for the latest published version via `cargo search`, then installs it via `cargo install symposium --force`. Also provides `re_exec()` which replaces the current process with the newly installed binary (Unix `exec`, spawn-and-exit on Windows) — used by the `auto-update = "on"` startup path. Contains `maybe_warn_for_update()` (sync, for the `warn` library path) and `maybe_check_for_update()` (async, for the binary `on` + re-exec path).

### `crate_command.rs` — crate source lookup

Contains `dispatch_crate()`, which resolves a crate's version and fetches its source code. Called by the CLI's `crate-info` command. Path dependencies are resolved to their local source directory via `WorkspaceCrate.path`.
