# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/cargo-agents.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines the user-wide `Config` (stored at `~/.symposium/config.toml`) with `[[agent]]` entries, logging, plugin sources, defaults, and `auto-update` (off/warn/on, default warn). Provides `plugin_sources()` to resolve the effective list of plugin source directories. The `workspace_deps(cwd)` factory is the standard way to create a `WorkspaceDeps` — it wires in `cargo_override` and `cache_dir` so callers get both the `SYMPOSIUM_CARGO` override and cross-invocation disk caching.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization command

Implements `cargo agents init`. Prompts for agents (or accepts `--add-agent`/`--remove-agent` flags), hook scope, auto-update behavior, and opt-in [telemetry](./telemetry.md); writes user config; and registers global hooks.

### `sync.rs` — synchronization command

Implements `cargo agents sync`. Scans workspace dependencies, finds applicable skills from plugin sources, and synchronizes them into each configured agent's skill directory. The core primitive is `sync_skill_dir(source_dir, dest_dir, project_root)`, shared by both the plugin-skill and user-authored-skill code paths. It copies the entire source directory (not just `SKILL.md`) and is change-aware: it compares source and destination content, only performing the delete-and-recopy when files actually differ, so the disk shows no modifications when nothing changed. A configurable debounce (`sync-debounce-secs`, default 5s, keyed on the `.symposium` marker's mtime) skips even the comparison for recently-synced skills. On each sync, scans every agent's skills parent directory and reaps any marker-bearing subdirectory it didn't install this time, leaving user-managed skills (which lack the marker) untouched. Writes a `.gitignore` with `*` only into individual skill directories (not parent directories like `.claude/` or `.claude/skills/`). Also provides `register_hooks()` for use by `init`, which registers only symposium's own global hook handler — individual plugin hooks are never written into agent configs.

Two entry points: `sync(sym, cwd)` for standalone CLI use (creates its own `WorkspaceDeps`) and `sync_with_deps(sym, deps)` for the hook pipeline (shares the cached workspace resolution with other hook stages).

`sync` takes an `UpdateLevel` that it threads into skill resolution (`skills_applicable_to`), controlling how aggressively `source.git` skill groups are re-fetched. Callers choose: the auto-sync path passes `Check` on `SessionStart` (refresh) and `None` otherwise (debounced); the binary's global `--update` flag feeds manual `cargo agents sync`.

### `plugins.rs` — plugin registry

Scans configured plugin source directories for TOML manifests and parses them into `Plugin` structs. Validation here turns the raw TOML into:
- `Installation` entries (optional `source`, optional `executable`/`script`, optional `args`, plus `requirements` and `install_commands`) collected on `Plugin.installations`. Inline installation references on hooks or other installations are *promoted* into synthetic `Installation` entries with derived names (`<hook>` for an inline `command`, `<owner>__req_<i>` for an inline requirement), so all references in the validated form are plain names.
- `Hook` entries with `command: String` (the name of an `Installation`) plus optional hook-level `executable` / `script` / `args`. Validation guarantees at most one of `executable`/`script` is set across hook + installation, and at most one layer sets `args`.

Also discovers standalone `SKILL.md` files not wrapped in a plugin. Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `installation.rs` — sources and acquisition

Defines `Source` (the `source = "..."`-tagged enum: `cargo`, `github`) and `acquire_source`, which downloads / installs / clones the source and returns an `AcquiredSource` whose `resolve_executable` / `resolve_script` methods turn a relative `executable`/`script` name into a concrete path. The `Runnable` enum (`Exec(PathBuf)` or `Script(PathBuf)`) is the final form a hook command resolves to. The `git` submodule handles GitHub tarball acquisition and caching.

`acquire_source` (and the main-crate `acquire_installation` wrapper) take an `UpdateLevel`. `None` serves the cache without touching the network; `Check`/`Fetch` re-resolve. Hook dispatch acquires with `None`; the `SessionStart` prewarm uses `Check`. The three source kinds:

- **crates.io cargo**: the resolved version is recorded in a `current` pointer (`<cache>/binaries/<crate>/current`). `None` reads the pointer and serves that version with **no crates.io query** — so a per-event dispatch never hits the registry. `Check`/`Fetch` query for the newest matching version, install into its version-keyed dir, and rewrite the pointer (so newly published versions are picked up at session start, not on every event). Only `Fetch` forces a same-version reinstall.
- **`cargo + git`**: the cache key folds in only the URL + user version, never the resolved commit, so a moved branch never invalidates it on its own. `Check`/`Fetch` resolve the remote `HEAD` with a cheap `git ls-remote` and compare against the commit recorded in a `.commit-sha` file in the cache dir; the binary is reinstalled (`cargo install --force`) **only when the SHA changed** (or `Fetch`, or the binary is missing). `None` never resolves the remote.
- **github** (script/subtree sources): honors the level directly via the git cache manager (freshness checks debounced to a 60s window under `None`).

Validates skill group source constraints at parse time: mutual exclusivity of `source.path`/`source.git`/`source = "crate"`, and the requirement that `source = "crate"` has at least one non-wildcard predicate.

### `crate_metadata.rs` — parse Cargo.toml metadata

Parses `[package.metadata.symposium]` from crate `Cargo.toml` files. Crate authors embed skill layout metadata so Symposium knows where to find skills (or which other crate to redirect to). Returns `SkillSource::Path(subdir)` or `SkillSource::Crate { name, version }` for redirects.

### `predicate.rs` — unified activation predicates

Defines one `Predicate` enum covering both crate-graph matching and runtime/environment gating, plus `PredicateSet` (a list ANDed together) and `PredicateContext` (the workspace crate list it evaluates against). Two surface syntaxes lower to the same tree:

- The **`crates`** field uses crate-atom syntax (`serde`, `serde>=1.0`, `*`) and lowers, via `CrateList`, to `crate(...)` / `crate(*)` predicates OR-combined into a single `any(...)` that is appended to the same list. So `crates` is sugar — there is no separate crate-predicate type.
- The **`predicates`** field uses function-call syntax: `crate(<atom>)`, `shell(<cmd>)` (verbatim arg, `sh -c`, exit 0 holds), `path_exists(<arg>)` (disk, then `$PATH` for bare names), `env(<name>[=<value>])`, and the combinators `not(<p>)`, `any(<p>, …)`, `all(<p>, …)`.

Each gated struct (plugin, skill group, skill, hook, MCP server, subcommand) stores a single merged `predicates: PredicateSet`. Evaluation is `PredicateSet::evaluate(ctx) -> bool`. For `source = "crate"`, `witness` / `union_matched_crates` return the concrete crates that participate in a *satisfying* evaluation (the fetch set): `crate(c)` contributes `c` when present, `any` unions its true children, `all` unions all children when all hold, and `not` contributes nothing. `collect_crate_names` (crates.io validation) walks all positions regardless. Plugin/group/skill/MCP predicates are evaluated at sync time; hook dispatch evaluates the plugin-level set (so a plugin's `crates` now gate its hooks) plus the hook-level set. Hook dispatch threads in the workspace crate list, but resolves it (running cargo) only when some plugin- or hook-level predicate references a *concrete* `crate(...)` — wildcard and env/shell/path predicates dispatch without a cargo query. See the [predicates reference](../reference/predicates.md).

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, and evaluates crate predicates at each level (plugin, group, skill) to determine which skills apply. For `source = "crate"` groups, resolves predicates to a matched crate set, fetches each crate's source via `RustCrateFetch`, reads `[package.metadata.symposium]` to determine skill paths, and follows redirects recursively with cycle detection and a depth limit of 10.

Each applicable skill carries a `SkillOrigin` describing *where its bytes live*, used at sync time for dedup and install-path disambiguation. What matters for identity is the on-disk location of the skill, not which plugin manifest pointed at it — two plugins in the same source pointing at the same skill bundle dedupe.
- `Crate { name, version }` — from a crate-source resolution (`source = "crate"`). Two `Crate` origins with the same `(name, version)` are the same logical skill, regardless of which plugin pointed at them.
- `Git { repo, commit_sha, skill_path }` — from a `source.git` group. Identity is the triple `(owner/repo, resolved commit SHA, SKILL.md path within the repo tree)`. Two plugins that pointed at the same repo via different URL forms (root URL vs. `tree/<ref>/<subpath>`) collapse to one install when they end up loading the same SKILL.md from the same commit; different SKILL.md files within one repo stay distinct.
- `Source { source_name, skill_path }` — from a plugin's `source.path` group, or from a standalone `SKILL.md` discovered in a registry source. `source_name` is the registry source's display name (e.g. `"user-plugins"`); `skill_path` is the SKILL.md's parent directory relative to the source root (canonicalized first, so `../`-laden joins collapse to the same string as a direct standalone walk).

`sync` prefers the plain `<agent-skills-dir>/<skill-name>/` and only falls back to `<skill-name>-<origin-disambiguator>/` when needed: when more than one origin claims the same skill name, or when the unsuffixed slot is already occupied by a user-managed directory (one without the `.symposium` marker). The disambiguator is human-readable for `Crate` (`<crate-name>-<version>`) and an 8-hex SHA-256 prefix for the other variants. The `.symposium` marker, wildcard `.gitignore`, and stale-cleanup walk all key on the marker file rather than directory name shape, so transitions between unsuffixed and suffixed names self-heal across syncs.

### `subcommand_dispatch.rs` — plugin-vended subcommands

Routes the `Commands::External` arm of clap's `allow_external_subcommands`. `find_subcommand` walks the `PluginRegistry`, applying plugin-level and subcommand-level crate predicates against the workspace, and returns the matched `(Plugin, Subcommand)` (or an error if more than one plugin claims the name). `dispatch_external` then looks up the named `Installation`, resolves it via `installation::resolve_runnable`, and spawns the child with stdio inherited — propagating the exit code as a `u8` so callers can convert to `ExitCode` (binary) or treat non-zero as an error (library). `applicable_subcommands` is the shared iterator over workspace-applicable plugin subcommands, reused by help rendering.

### `help_render.rs` — `--help` rendering

Renders `cargo agents --help` as two audience-grouped sections, "Commands for humans" and "Commands for agents", mixing built-in subcommands with plugin-vended ones filtered by the active workspace. Built-in audience comes from `cli::builtin_audience`; plugin subcommands come from `subcommand_dispatch::applicable_subcommands`.

`help_text` is the single help decision, shared by the binary and the test harness. clap's own help flag and help subcommand are disabled (in `cli::Cli`), `--help`/`-h` is a manual `global` bool, and the entry points parse with `try_parse_from` — so help is decided *after* parsing and argument order (`--help --quiet`) is irrelevant. It returns the top-level grouped help for no subcommand / `--help` / `-h` / the bare `help` keyword; for `<built-in> --help` it re-renders clap's own per-command help by walking clap's command tree (so required-arg commands like `crate-info`, required-subcommand groups like `plugin`, and nested commands like `plugin list` all work); a plugin `<name> --help` returns `None` so dispatch forwards `--help` to the child.

`render` builds the grouped text by slicing clap's rendered help — keeping the header (before `Commands:`) and the options block (from `Options:` on) and hand-rendering only the two section headings between them. If a slice marker is missing (clap format drift), it falls back to clap's unmodified help rather than panicking.

### `hook.rs` — hook handling

Handles the hook pipeline: parse agent wire-format input → auto-sync → builtin dispatch → plugin hook dispatch → serialize output. A single `WorkspaceDeps` (created via `sym.workspace_deps(cwd)`) is threaded through all stages — `run_auto_sync`, `dispatch_builtin`, `dispatch_plugin_hooks`, and the `SessionStart` prewarm. In-process, at most one `cargo metadata` invocation occurs per hook call (down from up to three previously). Across invocations, the disk cache means zero `cargo metadata` calls when `Cargo.lock` hasn't changed — the common case for `PreToolUse` hooks.

`run_auto_sync` takes a `session_start` flag: on `SessionStart` it skips the `Cargo.lock` freshness gate and syncs with `UpdateLevel::Check` (so upstream skill/source changes land once per session); every other event keeps the gated, `UpdateLevel::None` path. The matching `ensure_plugin_sources` refresh level is decided in the binary entry point from the same event. `SessionStart` additionally runs `prewarm_hook_sources` (best-effort, gated by `auto-sync`): it walks every applicable plugin's hooks and *refreshes* each installation's already-cached source via `refresh_installation_if_present` (`UpdateLevel::Check`). This is what keeps hook *binaries/scripts* (not just manifests) current once per session — in particular the only path that re-pulls a `cargo + git` hook binary whose branch moved — so the dispatch path can keep acquiring with `None` (cache/debounced) and pay no per-event network cost. It is **refresh-only**: a source that was never acquired is left alone (it installs lazily on first dispatch), so `SessionStart` never eagerly installs a tool a hook may never use.

Builtin dispatch currently only acts on `SessionStart`, where `handle_session_start` composes two independently-computed `additionalContext` fragments: a `discovery_hint` (suggests `cargo agents --help` when the workspace exposes applicable plugin subcommands, reusing `subcommand_dispatch::applicable_subcommands`) and an `update_nudge` (the throttled self-update warning); the discovery hint is not gated behind the update-check throttle. The plugin dispatch path matches plugin `Hook`s against the event, selects the best format for each plugin (native match > symposium > single-other-agent fallback), builds a `ResolvedHook` per match (looking up the named installations on the plugin), then for each `ResolvedHook`: acquires its `requirements` (best-effort), runs `install_commands` after the source step, picks a `Runnable` from (hook-or-install) `executable`/`script`, and spawns it (binary directly for `Exec`, via `sh <path>` for `Script`). Input is delivered in the selected format; output is converted back to the agent's wire format before returning.

### `state.rs` — persistent state

Manages `state.toml` in the config directory. Tracks the semver of the binary that last touched the directory (for future migration hooks) and the timestamp of the last update check (to throttle crates.io queries to once per 24 hours). `ensure_current()` is called on startup to silently stamp the current version. `should_check_for_update()` / `record_update_check()` gate the auto-update flow.

### `telemetry.rs` — opt-in usage telemetry

Implements the local, opt-in [telemetry](./telemetry.md) event log under `<config-dir>/telemetry/`, one JSONL file per UTC day. Off by default; gated by `[telemetry] enabled`. A `TelemetryEvent` is an `at` timestamp plus a kind-tagged `EventKind` (`session_start` / `user_prompt` / `tool_use`), serialized one per line. `record` / `record_kind` append an event; `roll_off` deletes files older than `RETENTION_DAYS` (30); `read_events` / `recent_events` read them back; `usage` + `status_text` back `telemetry status`; `recent_events` backs `telemetry show`. Events are anonymous by construction — no prompt text, command lines, or file paths. Every write path is best-effort — failures are logged and swallowed so a hook is never broken. The recording entry points are not yet called from the hook pipeline, so no events are produced today even when telemetry is enabled.

### `report.rs` — structured report layer

Provides user-facing output for all commands via a custom tracing layer. Commands emit `tracing::info!` or `tracing::debug!` events with a `report = %ReportEvent::Variant { ... }` field; the `ReportLayer` intercepts these and renders them based on mode:

- `Normal` — prints `format_human()` to stdout (default for most commands)
- `Verbose` (`-v`) — prints all events (info + debug) to stderr
- `Json` (`--json`) — accumulates events in a buffer, drained as a JSON array at the end

The `ReportEvent` enum is the stable schema — `#[derive(Serialize, Deserialize)]` with `#[serde(tag = "kind")]`. Each variant carries the fields needed to render both human and JSON forms. The `Display` impl serializes to JSON (for passing through tracing's `%` formatter), and `format_human()` renders the pretty-printed form.

The layer is always installed by the binary. Commands that want output simply emit report events at the appropriate tracing level (info for actions, debug for decision trace). The `--json` flag also suppresses the `Output`-based messages and drains the JSON buffer at exit.

### `self_update.rs` — self-update

Implements `cargo agents self-update`. Queries the registry for the latest published version via `cargo search`, then installs it via `cargo install symposium --force`. Also provides `re_exec()` which replaces the current process with the newly installed binary (Unix `exec`, spawn-and-exit on Windows) — used by the `auto-update = "on"` startup path. Contains `maybe_warn_for_update()` (sync, for the `warn` library path) and `maybe_check_for_update()` (async, for the binary `on` + re-exec path).

### `crate_command.rs` — crate source lookup

Contains `dispatch_crate()`, which resolves a crate's version and fetches its source code. Called by the CLI's `crate-info` command. Path dependencies are resolved to their local source directory via `WorkspaceCrate.path`.
