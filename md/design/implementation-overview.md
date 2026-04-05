# Implementation overview

Symposium is a Rust crate with both a library (`src/lib.rs`) and binary (`src/main.rs`). The source is in `src/`:

| File | Purpose |
|------|---------|
| `lib.rs` | Library root, re-exports all modules for integration tests. |
| `main.rs` | CLI entry point using clap. Defines subcommands: `start`, `tutorial`, `mcp`, `hook`, `crate`, `plugin`. Creates `Symposium` context, initializes logging, ensures plugin sources. |
| `config.rs` | Reads `~/.symposium/config.toml`. Defines `Settings` (TOML config), `Config` (settings + resolved paths), and `Symposium` (Config + lazily-opened DB handle via `Deref<Target=Config>`). Two constructors: `from_environment()` (production) and `from_dir(path)` (tests). |
| `dispatch.rs` | Shared dispatch logic for CLI and MCP. Both route through `dispatch()` which handles `start`, `crate`, and `help` commands. |
| `hook.rs` | Handles hook events. Built-in handlers for PostToolUse (activation detection) and UserPromptSubmit (crate mention scanning). Plugin hook dispatch for PreToolUse. Delegates all DB operations to `state::session`. |
| `state/mod.rs` | SQLite state layer via toasty. DB at `<config_dir>/state.0.sqlite`. Shared `open_db()` function. |
| `state/session.rs` | Per-session state: `SessionState`, `SkillActivation`, `SkillNudge` models + DB helpers (`record_activation`, `increment_prompt_count`, `compute_nudges`). |
| `workspace.rs` | On-demand computation of available skills for the workspace. `compute_available_skills()` scans workspace deps and resolves matching plugin skill groups (no caching). |
| `tutorial.rs` | Renders the tutorial template (`md/tutorial.md`). |
| `mcp.rs` | MCP server over stdio using `sacp`. Exposes a single `rust` tool taking `args: Vec<String>`, dispatched through the shared dispatch layer. |
| `crate_sources/` | Crate source fetching: version resolution, cache lookup, download+extraction. |
| `plugins.rs` | Plugin registry: loads TOML manifests from configured plugin sources, produces `Vec<Plugin>` as a table of contents. Defines `SkillGroup`, `PluginSource`, `Hook` types. Does not load skill content — that is handled by the skills layer. |
| `git_source.rs` | GitHub URL parsing, API client, and plugin cache manager. Downloads tarballs, extracts subdirectories, caches under `~/.symposium/cache/` with commit SHA freshness checking. |
| `skills.rs` | Skill model, frontmatter parsing, discovery, and crate advice output. Given loaded plugins, resolves skill group sources (fetching from git if needed), discovers `SKILL.md` files, evaluates `crates` predicates, and formats output. Skills follow the [agentskills.io](https://agentskills.io/specification.md) format. |
| `predicate.rs` | Parser and evaluator for crate predicates. Supports crate atoms (`serde`, `tokio>=1.0`) with optional version constraints. |

## Key dependencies

- **sacp / sacp-tokio** — MCP server implementation
- **clap** — CLI argument parsing
- **toasty** (with `sqlite` feature) — ORM for SQLite state tracking
- **tracing / tracing-subscriber** — Structured logging to `~/.symposium/logs/`
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
cargo run -- start             # Rust guidance + crate skill list
cargo run -- tutorial          # print the tutorial (static only)
cargo run -- hook pre-tool-use # reads event JSON from stdin
cargo run -- crate tokio       # find crate source location
cargo run -- crate --list      # list skills available for workspace crates
cargo run -- plugin sync       # refresh plugin sources
```

## Claude Code plugin structure

The plugin at `agent-plugins/claude-code/` contains:

- `.claude-plugin/plugin.json` — Plugin manifest
- `scripts/symposium.sh` — Bootstrap script shared by skills and hooks
- `skills/rust/SKILL.md` — Static skill telling the agent to run `symposium start`
- `hooks/hooks.json` — Hook configuration (registers `PreToolUse`, `PostToolUse`, and `UserPromptSubmit` hooks)

## Integration tests

Integration tests are in `tests/` using composable fixtures:

- `tests/testlib/mod.rs` — `TestContext` harness with `with_fixture()` helper
- `tests/fixtures/workspace0/` — Minimal Cargo workspace with tokio/serde deps
- `tests/fixtures/plugins0/` — Local plugin with serde skill, no network required
- `tests/integration.rs` — Smoke tests for dispatch and hook handling
