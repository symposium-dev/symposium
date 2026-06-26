# Key modules

Symposium is a Rust crate with both a library (`src/lib.rs`) and a binary (`src/bin/cargo-agents.rs`). The library re-exports all modules so that integration tests can access internals.

### `config.rs` — application context

Everything hangs off the `Symposium` struct, which wraps the parsed `Config` with resolved paths for config, cache, and log directories. Two constructors: `from_environment()` for production and `from_dir()` for tests.

Defines the user-wide `Config` (stored at `~/.symposium/config.toml`) with `[[agent]]` entries, logging, installed registry sources (`[installed]` / `[installed.crates]`), `[discovery.allow]` / `[discovery.deny]` policy, and `auto-update` (off/warn/on, default on). Legacy `[defaults]` and `[[plugin-source]]` config are rejected by the schema. The `workspace_deps(cwd)` factory is the standard way to create a `WorkspaceDeps` — it wires in `cargo_override` and `cache_dir` so callers get both the `SYMPOSIUM_CARGO` override and cross-invocation disk caching.

### `agents.rs` — agent abstraction

Centralizes agent-specific knowledge: hook registration file paths, skill installation directories, and hook registration logic for each supported agent (Claude Code, GitHub Copilot, Gemini CLI, Codex CLI, Kiro, OpenCode, Goose). Handles the differences between agents — e.g., Claude Code uses `.claude/skills/` and Kiro uses `.kiro/skills/`, while Copilot, Gemini, Codex, OpenCode, and Goose use the vendor-neutral `.agents/skills/`. OpenCode and Goose are skills-only agents (no hook registration).

### `init.rs` — initialization command

Implements `cargo agents init`. Prompts for agents (or accepts `--add-agent`/`--remove-agent` flags), writes user config (including the default `symposium-recommendations` installed crate), and registers global hooks.

### `install.rs` — install/uninstall commands

Implements `cargo agents install` and `cargo agents uninstall`. Parses `<CRATE>[@<VERSION>]`, `--git`, and `--path` forms, rejects mixed source forms, and mutates `[installed.crates]`, `installed.git`, or `installed.paths` deterministically. Sync consumes these entries through the resolved source graph.

### `status.rs` — status command

Implements `cargo agents status`. Loads installed crates and workspace deps, evaluates predicates, and reports which plugins/skills are active or inactive and why.

### `sync.rs` — synchronization command

Implements `cargo agents sync`. Resolves the source graph (`resolve_sync_sources`) from installed sources (`[installed.crates]`/`installed.paths`/`installed.git`) plus the workspace root (when it has `SYMPOSIUM.toml`) and members, then expands the graph via `expand_source_graph` to resolve discovery-allowed dependency candidates and recursive `[[plugins]] source.git`/`source.crate` declarations. After expansion, calls `load_registry_from_graph` to build the plugin registry with provenance-stamped plugins. Discovers applicable skills, and synchronizes them into each configured agent's skill directory. The core primitive is `sync_skill_dir(source_dir, dest_dir, project_root)`, shared by both the plugin-skill and user-authored-skill code paths. It copies the entire source directory (not just `SKILL.md`) and is change-aware: it compares source and destination content, only performing the delete-and-recopy when files actually differ, so the disk shows no modifications when nothing changed. A configurable debounce (`sync-debounce-secs`, default 5s, keyed on the `.symposium` marker's mtime) skips even the comparison for recently-synced skills. On each sync, scans every agent's skills parent directory and reaps any marker-bearing subdirectory it didn't install this time, leaving user-managed skills (which lack the marker) untouched. Writes a `.gitignore` with `*` only into individual skill directories (not parent directories like `.claude/` or `.claude/skills/`). Also provides `register_hooks()` for use by `init`, which registers only symposium's own global hook handler — individual plugin hooks are never written into agent configs.

Two entry points: `sync(sym, cwd)` for standalone CLI use (creates its own `WorkspaceDeps`) and `sync_with_deps(sym, deps)` for the hook pipeline (shares the cached workspace resolution with other hook stages).

### `plugins.rs` — plugin registry

Scans plugin source roots for `SYMPOSIUM.toml` manifests and parses them into `Plugin` structs. A source root is always a plugin boundary: if `$ROOT/SYMPOSIUM.toml` exists it is loaded, otherwise an empty root manifest is synthesized. Every manifest defaults to `where.crates = ["*"]`, two implicit skill groups (`source.path = "skills"` and workspace-gated `source.path = ".agents/skills"`), and an implicit `[[plugins]] source.path = "."` that recursively searches for nested manifests. Nested manifests are independent plugins, not children owned by the parent. `defaults.skills = false` suppresses the implicit skill groups; `defaults.plugins = false` suppresses the implicit nested-manifest search. `[[plugins]] source.git` and `source.crate` declarations are parsed and retained for recursive registry-source resolution during graph expansion. Manifests may also declare `[discovery.allow]` / `[discovery.deny]` policy that contributes to the `CollectedPolicy` used during graph expansion.

The registry entry point is `load_registry_from_graph(graph)`, which stamps each `ParsedPlugin` with the source node's provenance set so `workspace()`, `dependency()`, and `installed()` predicates evaluate correctly.

Validation turns the raw TOML into:
- `Installation` entries (optional `source`, optional `executable`/`script`, optional `args`, plus `requirements` and `install_commands`) collected on `Plugin.installations`. Inline installation references on hooks or other installations are *promoted* into synthetic `Installation` entries with derived names (`<hook>` for an inline `command`, `<owner>__req_<i>` for an inline requirement), so all references in the validated form are plain names. Implicit installations from binary targets are merged in.
- `Hook` entries with `command: String` (the name of an `Installation`) plus optional hook-level `executable` / `script` / `args`. Validation guarantees at most one of `executable`/`script` is set across hook + installation, and at most one layer sets `args`.
- `PluginSourceDecl` entries retained from `[[plugins]] source.path`, `source.git`, and `source.crate`. Path declarations drive in-source recursive manifest discovery; git and crate declarations are resolved during `expand_source_graph`, which iterates until the graph converges.

Returns a `PluginRegistry` — a table of contents that doesn't load skill content.

### `installation.rs` — sources and acquisition

Defines `Source` (the `source = "..."`-tagged enum: `cargo`, `github`, `binary`) and `acquire_source`, which downloads / installs / clones the source and returns an `AcquiredSource` whose `resolve_executable` / `resolve_script` methods turn a relative `executable`/`script` name into a concrete path. The `Runnable` enum (`Exec(PathBuf)` or `Script(PathBuf)`) is the final form a hook command resolves to. The `git` submodule handles GitHub tarball acquisition and caching.

Validates skill group source constraints at parse time: mutual exclusivity of `source.path`/`source.git`.

### `predicate.rs` — unified activation predicates

Defines one `Predicate` enum covering both crate-graph matching and runtime/environment gating, plus `PredicateSet` (a list ANDed together) and `PredicateContext` (the workspace crate list it evaluates against). Two surface syntaxes lower to the same tree:

- The **`crates`** field uses crate-atom syntax (`serde`, `serde>=1.0`, `*`) and lowers, via `CrateList`, to `crate(...)` / `crate(*)` predicates OR-combined into a single `any(...)` that is appended to the same list. So `crates` is sugar — there is no separate crate-predicate type.
- The **`predicates`** field uses function-call syntax: `crate(<atom>)`, `shell(<cmd>)` (verbatim arg, `sh -c`, exit 0 holds), `path_exists(<arg>)` (disk, then `$PATH` for bare names), `env(<name>[=<value>])`, the source-context predicates `workspace()`, `dependency()`, `installed()` (evaluate against the non-exclusive provenance set on `PredicateContext`, updated per plugin before evaluation), and the combinators `not(<p>)`, `any(<p>, …)`, `all(<p>, …)`.

Manifest authors may use either legacy top-level `crates` / `predicates` fields or the registry-ready `where.crates` / `where.predicates` table on plugins, skill groups, hooks, MCP servers, subcommands, and `[[plugins]]` declarations. Validation lowers both shapes into the same `PredicateSet`.

Each gated struct (plugin, skill group, skill, hook, MCP server, subcommand) stores a single merged `predicates: PredicateSet`. Evaluation is `PredicateSet::evaluate(ctx) -> bool`. `collect_crate_names` (crates.io validation) walks all positions regardless. Plugin/group/skill/MCP predicates are evaluated at sync time; hook dispatch evaluates the plugin-level set (so a plugin's `crates` now gate its hooks) plus the hook-level set. Hook dispatch threads in the workspace crate list, but resolves it (running cargo) only when some plugin- or hook-level predicate references a *concrete* `crate(...)` — wildcard and env/shell/path predicates dispatch without a cargo query.

Source-context predicates (`workspace()`, `dependency()`, `installed()`) evaluate against a `BTreeSet<SourceProvenance>` on `PredicateContext`. Each evaluation site (`skills_applicable_to`, `dispatched_hooks_for_payload`, `applicable_subcommands`, and the MCP collection loop in sync) sets the provenance from `ParsedPlugin::source_provenance` before evaluating that plugin's predicates. The provenance is non-exclusive — a source can simultaneously be `installed`, `workspace`, and `dependency` — so all three predicates can be true for the same plugin. See the [predicates reference](../reference/predicates.md).

### `skills.rs` — skill resolution and matching

Given a `PluginRegistry` and workspace dependencies, this module resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, and evaluates crate predicates at each level (plugin, group, skill) to determine which skills apply.

Each applicable skill carries a `SkillOrigin` describing *where its bytes live*, used at sync time for dedup and install-path disambiguation. What matters for identity is the on-disk location of the skill, not which plugin manifest pointed at it — two plugins in the same source pointing at the same skill bundle dedupe.
- `Git { repo, commit_sha, skill_path }` — from a `source.git` group. Identity is the triple `(owner/repo, resolved commit SHA, SKILL.md path within the repo tree)`. Two plugins that pointed at the same repo via different URL forms (root URL vs. `tree/<ref>/<subpath>`) collapse to one install when they end up loading the same SKILL.md from the same commit; different SKILL.md files within one repo stay distinct.
- `Source { source_name, skill_path }` — from a plugin's `source.path` group. `source_name` is the crate's display name; `skill_path` is the SKILL.md's parent directory relative to the crate root (canonicalized first, so `../`-laden joins collapse to the same string).

`sync` prefers the plain `<agent-skills-dir>/<skill-name>/` and only falls back to `<skill-name>-<origin-disambiguator>/` when needed: when more than one origin claims the same skill name, or when the unsuffixed slot is already occupied by a user-managed directory (one without the `.symposium` marker). The disambiguator is an 8-hex SHA-256 prefix over the skill origin. The `.symposium` marker, wildcard `.gitignore`, and stale-cleanup walk all key on the marker file rather than directory name shape, so transitions between unsuffixed and suffixed names self-heal across syncs.

### `subcommand_dispatch.rs` — plugin-vended subcommands

Routes the `Commands::External` arm of clap's `allow_external_subcommands`. `find_subcommand` walks the `PluginRegistry`, applying plugin-level and subcommand-level crate predicates against the workspace, and returns the matched `(Plugin, Subcommand)` (or an error if more than one plugin claims the name). `dispatch_external` then looks up the named `Installation` (including implicit installations from binary targets), resolves it via `installation::resolve_runnable`, and spawns the child with stdio inherited — propagating the exit code as a `u8` so callers can convert to `ExitCode` (binary) or treat non-zero as an error (library). `applicable_subcommands` is the shared iterator over workspace-applicable plugin subcommands, reused by help rendering.

### `help_render.rs` — `--help` rendering

Renders `cargo agents --help` as two audience-grouped sections, "Commands for humans" and "Commands for agents", mixing built-in subcommands with plugin-vended ones filtered by the active workspace. Built-in audience comes from `cli::builtin_audience`; plugin subcommands come from `subcommand_dispatch::applicable_subcommands`.

`help_text` is the single help decision, shared by the binary and the test harness. clap's own help flag and help subcommand are disabled (in `cli::Cli`), `--help`/`-h` is a manual `global` bool, and the entry points parse with `try_parse_from` — so help is decided *after* parsing and argument order (`--help --quiet`) is irrelevant. It returns the top-level grouped help for no subcommand / `--help` / `-h` / the bare `help` keyword; for `<built-in> --help` it re-renders clap's own per-command help by walking clap's command tree (so required-arg commands like `crate-info`, required-subcommand groups like `plugin`, and nested commands like `plugin validate` all work); a plugin `<name> --help` returns `None` so dispatch forwards `--help` to the child.

`render` builds the grouped text by slicing clap's rendered help — keeping the header (before `Commands:`) and the options block (from `Options:` on) and hand-rendering only the two section headings between them. If a slice marker is missing (clap format drift), it falls back to clap's unmodified help rather than panicking.

### `hook.rs` — hook handling

Handles the hook pipeline: parse agent wire-format input → auto-sync → builtin dispatch → plugin hook dispatch → serialize output. A single `WorkspaceDeps` (created via `sym.workspace_deps(cwd)`) is threaded through all stages — `run_auto_sync`, `dispatch_builtin`, and `dispatch_plugin_hooks`. In-process, at most one `cargo metadata` invocation occurs per hook call (down from up to three previously). Across invocations, the disk cache means zero `cargo metadata` calls when `Cargo.lock` hasn't changed — the common case for `PreToolUse` hooks.

Builtin dispatch currently only acts on `SessionStart`, where `handle_session_start` composes two independently-computed `additionalContext` fragments: a `discovery_hint` (suggests `cargo agents --help` when the workspace exposes applicable plugin subcommands, reusing `subcommand_dispatch::applicable_subcommands`) and an `update_nudge` (the throttled self-update warning); the discovery hint is not gated behind the update-check throttle. The plugin dispatch path matches plugin `Hook`s against the event, selects the best format for each plugin (native match > symposium > single-other-agent fallback), builds a `ResolvedHook` per match (looking up the named installations on the plugin), then for each `ResolvedHook`: acquires its `requirements` (best-effort), runs `install_commands` after the source step, picks a `Runnable` from (hook-or-install) `executable`/`script`, and spawns it (binary directly for `Exec`, via `sh <path>` for `Script`). Input is delivered in the selected format; output is converted back to the agent's wire format before returning.

### `state.rs` — persistent state

Manages `state.toml` in the config directory. Tracks the semver of the binary that last touched the directory (for future migration hooks) and the timestamp of the last update check (to throttle crates.io queries to once per 24 hours). `ensure_current()` is called on startup to silently stamp the current version. `should_check_for_update()` / `record_update_check()` gate the auto-update flow.

### `report.rs` — structured report layer

Provides user-facing output for all commands via a custom tracing layer. Commands emit `tracing::info!` or `tracing::debug!` events with a `report = %ReportEvent::Variant { ... }` field; the `ReportLayer` intercepts these and renders them based on mode:

- `Normal` — prints `format_human()` to stdout (default for most commands)
- `Verbose` (`-v`) — prints all events (info + debug) to stderr
- `Json` (`--json`) — accumulates events in a buffer, drained as a JSON array at the end

The `ReportEvent` enum is the stable schema — `#[derive(Serialize, Deserialize)]` with `#[serde(tag = "kind")]`. Each variant carries the fields needed to render both human and JSON forms. The `Display` impl serializes to JSON (for passing through tracing's `%` formatter), and `format_human()` renders the pretty-printed form.

The layer is always installed by the binary. Commands that want output simply emit report events at the appropriate tracing level (info for actions, debug for decision trace). The `--json` flag also suppresses the `Output`-based messages and drains the JSON buffer at exit.

### `self_update.rs` — self-update

Implements `cargo agents self-update`. Queries the registry for the latest published version via `cargo search`, then installs it via `cargo install symposium --force`. Also provides `re_exec()` which replaces the current process with the newly installed binary (Unix `exec`, spawn-and-exit on Windows) — used by the `auto-update = "on"` startup path. Contains `maybe_warn_for_update()` (sync, for the `warn` library path) and `maybe_check_for_update()` (async, for the binary `on` + re-exec path).

### `crate_sources/` — source acquisition

Contains `RustCrateFetch`, used by `crate-info`, plus the registry-ready `SourceRegistryResolver`. The resolver accepts `RegistrySourceSpec::Path`, `::Git`, and `::Crate` and returns concrete `ResolvedSourceRoot` directories for discovery. Direct path sources canonicalize local directories without network I/O; direct git sources reuse `symposium-install`'s `GitCacheManager`; crate-registry sources render Cargo dependency-table specs into a temporary probe package and ask Cargo to resolve registry, git, or path-backed crates.

Also defines `ResolvedSourceGraph`, a pre-discovery graph that combines installed sources, workspace sources, discovery-allowed dependency sources, and recursive plugin-source declarations. Nodes dedupe by canonical source path and keep non-exclusive provenance flags (`installed`, `workspace`, `dependency`) plus human-readable reasons. Sync uses `load_registry_from_graph` to build the plugin registry from resolved source nodes, stamping each `ParsedPlugin` with the node's provenance. Resolution of installed crate-registry sources is best-effort: failures are logged and skipped rather than aborting sync. `expand_source_graph` uses a worklist: it collects discovery policy from already-resolved plugins, evaluates workspace dependency candidates against the policy, and resolves recursive `[[plugins]] source.git`/`source.crate` declarations until the graph converges. Recursive source edges inherit the declaring source's full provenance set.

### `discovery.rs` — discovery policy evaluation

Implements the allow/deny specificity rules for workspace dependency auto-discovery. `CollectedPolicy` aggregates discovery rules from user config and installed plugin manifests. `DiscoveryCandidate` represents a candidate source (currently crate-registry candidates from workspace dependencies). The policy evaluator applies specificity-based matching: a specific rule beats a wildcard; if allow and deny have the same specificity, deny wins; and the default (no matching rule) is deny. Helper functions build candidates from workspace crates and resolve allowed candidates into source specs.

### `crate_command.rs` — crate source lookup

Contains `dispatch_crate()`, which resolves a crate's version and fetches its source code through the `RustCrateFetch` compatibility wrapper. Called by the CLI's `crate-info` command. Path dependencies are resolved to their local source directory via `WorkspaceCrate.path`.
