# WIP: Rethink LLM Workflow — Skill + Hooks + State

## Goal

Redesign when and how Symposium surfaces guidance to LLMs. Instead of a passive skill document, create an active workflow:

1. A **static skill** tells the agent to call `symposium start` before writing Rust
2. `symposium start` returns **general Rust guidance + a dynamic crate skill list** for the workspace
3. **Hooks** proactively nudge the agent when it's using a crate with available skills it hasn't loaded
4. A **SQLite database** (via toasty) tracks session state across hook invocations

## Architecture

```
Static skill (in plugin)
  "Critical. Invoke `symposium start` before authoring Rust."

  → agent invokes `symposium start` (CLI) or `rust` tool with `["start"]` (MCP)

symposium start / rust ["start"]
  → baked-in Rust guidance
  → workspace crate skill list (dynamic, from crate --list)

  → agent sees available crate, invokes

symposium crate <name> (CLI) or rust ["crate", "<name>"] (MCP)
  → returns skill content (no DB writes — activation is recorded by the hook layer)

Meanwhile, hooks fire:

All hooks (entry point):
  → check Cargo.lock mtime against WorkspaceCache in DB
  → if changed (or first run): re-scan workspace deps + plugins,
    upsert AvailableSkill rows (crate_name, skill_dir_path) into DB
  → if unchanged: skip (DB already has correct data)

PostToolUse hook (new built-in)
  → fires after tool execution completes; payload includes tool_response
  → if agent successfully invoked `symposium crate` (Bash) or `rust` tool (MCP)
    → record SkillActivation for the requested crate (session_id from hook payload)
  → if Read/Bash successfully accessed a path matching an AvailableSkill.skill_dir_path (DB lookup)
    → record SkillActivation for that skill

PreToolUse hook (existing)
  → plugin hook dispatch only (no built-in activation logic)

UserPromptSubmit hook
  → scans prompt for crate names with available skills (from DB)
  → checks DB: has this skill been activated in this session?
  → if gap → injects additionalContext nudge via hookSpecificOutput
```

## MCP Tool (after redesign)

One tool: **`rust`** — takes a vector of strings, dispatched the same way as the CLI.

- Description: *"Critical. Invoke this tool with `["start"]` as argument before authoring or working with Rust code."*
- Input: `args: Vec<String>` — passed directly to the same dispatch logic as the CLI subcommands
- Examples:
  - `["start"]` → returns baked-in Rust guidance + dynamic crate skill list
  - `["crate", "--list"]` → lists workspace crates with available skills
  - `["crate", "tokio"]` → returns crate skill content
  - `["crate", "serde", "--version", "1.0"]` → specific version

This replaces the current `rust` tool (single `command` string) and `crate` tool (tagged enum). The MCP layer is a thin shim over the shared CLI dispatch code.

## Hook Output Format

Hooks return JSON on stdout. The key mechanism is `additionalContext` inside `hookSpecificOutput`, which injects text into the LLM's conversation context:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "The `tokio` crate has available guidance...\n\nTo load it: symposium crate tokio"
  }
}
```

## Hook Payloads We Use

**UserPromptSubmit** — fires on every user prompt:
- `session_id` — scope state per conversation
- `prompt` — scan for crate name mentions
- `cwd` — workspace root for crate resolution

**PostToolUse** — fires after tool execution completes:
- `session_id`, `cwd` — same as other hooks
- `tool_name` — the tool that executed (e.g., `"Bash"`, `"Read"`, MCP tool names)
- `tool_input` — the parameters passed to the tool
- `tool_response` — the result returned by the tool (confirms success before recording activation)
- Detect when the agent successfully invoked `symposium crate <name>` (via Bash) or the `rust` MCP tool with `["crate", "<name>"]` — record a `SkillActivation` using `session_id` from the hook payload. This is the **only** path that records activations (the `symposium crate` command itself does not write to the DB).
- Detect when the agent successfully read a known skill directory path (via Read or Bash tool). If the tool target matches a known skill directory, record a `SkillActivation` for that skill.

**PreToolUse** (existing) — keep for plugin hook dispatch only. No built-in activation logic.

## Configuration

New `[hooks]` section in `~/.symposium/config.toml`:

```toml
[hooks]
# Number of prompts before re-nudging about an unloaded crate skill.
# Set to 0 to disable nudges entirely.
nudge-interval = 50  # default
```

When `nudge-interval = 0`, the `UserPromptSubmit` hook exits immediately (no DB access, no prompt scanning).

## SQLite State (via toasty)

Database location: `~/.symposium/state.0.sqlite`

The filename encodes the schema version (`.0.`). On breaking schema changes, bump to `.1.sqlite` etc. — no migration code needed, old files are simply ignored. We can add cleanup of old versions later if desired.

```rust
#[derive(Debug, toasty::Model)]
struct SkillActivation {
    #[key]
    #[auto]
    id: i64,

    #[index]
    session_id: String,

    crate_name: String,

    activated_at: String,
}

/// Tracks that we already nudged the agent about a crate skill in this session,
/// so we don't repeat ourselves. Includes the prompt number at which the nudge
/// was sent, so we can re-nudge after enough prompts have elapsed.
#[derive(Debug, toasty::Model)]
struct SkillNudge {
    #[key]
    #[auto]
    id: i64,

    #[index]
    session_id: String,

    crate_name: String,

    /// The prompt count at which this nudge was sent.
    at_prompt: i64,
}

/// Tracks per-session prompt count.
#[derive(Debug, toasty::Model)]
struct SessionState {
    #[key]
    session_id: String,

    /// Incremented on each UserPromptSubmit.
    prompt_count: i64,
}

/// Cached workspace deps, keyed by cwd. Refreshed when Cargo.lock mtime changes.
#[derive(Debug, toasty::Model)]
struct WorkspaceCache {
    #[key]
    cwd: String,

    /// Cargo.lock mtime (seconds since epoch) at time of caching.
    cargo_lock_mtime: i64,

    /// JSON array of {name, version} for workspace deps with available skills.
    deps_json: String,
}

/// Available skills for the current workspace, populated during the
/// Cargo.lock mtime refresh step at hook entry. Upserted (no-op if
/// already present). Queried by PostToolUse for path-based activation
/// and by UserPromptSubmit for nudge candidates.
#[derive(Debug, toasty::Model)]
struct AvailableSkill {
    #[key]
    #[auto]
    id: i64,

    /// The cwd this skill is relevant to.
    #[index]
    cwd: String,

    /// Crate name (e.g., "tokio").
    crate_name: String,

    /// Resolved directory path containing the SKILL.md file.
    skill_dir_path: String,
}
```

### Session ID resolution

Skill activations are only recorded by the hook layer, which always has `session_id` from the hook payload. The `symposium crate` command itself does not write to the DB — it just returns content.

**No GC for now** — sessions accumulate, revisit later.

## Implementation Steps

### Step 0: Replace global config with `Symposium` context struct

- Create a `Symposium` struct that owns `Config`, `config_dir`, `cache_dir`, and later the DB handle
- Two constructors:
  - `Symposium::from_environment()` — production path, resolves `SYMPOSIUM_HOME` / `XDG_*` / `~/.symposium`
  - `Symposium::from_dir(path)` — test path, everything rooted under a single directory
- Remove the thread-local `CONFIG` and free functions (`config_dir()`, `cache_dir()`, `plugin_sources()`)
- Thread `&Symposium` through all call sites: `plugins.rs`, `skills.rs`, `hook.rs`, `mcp.rs`, `git_source.rs`, `crate_sources/`
- This is a prerequisite for testability — without it, tests can't run in parallel or isolate state

### Step 1: Unify MCP and CLI dispatch

- Refactor `mcp.rs` to expose a single `rust` tool that takes `args: Vec<String>`
- The MCP tool passes `args` to the same dispatch logic used by CLI subcommands
- Remove the separate `crate` MCP tool and the `RustToolInput { command }` / `CrateToolInput` enums
- Extract shared dispatch logic so both CLI `main()` and MCP tool handler call the same code

### Step 2: Refactor hook runner to typed input/output

- Refactor `hook::run()`: currently returns `ExitCode` and only spawns plugin subprocesses
- Define typed `HookInput` / `HookOutput` structs for built-in hook behavior
- `HookOutput` includes optional `additionalContext` for injecting text into the LLM conversation
- Separate plugin-hook dispatch (spawn commands) from built-in hook behavior (compute + return struct)
- The caller serializes `HookOutput` to JSON and prints to stdout
- This structured API is what the integration test harness will use (`test.invoke_hook()` takes typed input, returns typed output)

### Step 3: Build integration test infrastructure

- Create `tests/testlib/` with the `TestContext` harness and `with_fixture()` helper
- `with_fixture()` takes an array of fixture names, overlays them into a single tempdir
- Create initial fixture fragments in `tests/fixtures/` (e.g., `workspace0` with `Cargo.toml`/`Cargo.lock`/`src/lib.rs`, and `plugins0` with local plugin manifests + skill files)
- `TestContext` wraps `Symposium::from_dir(tempdir)` with the fixtures overlaid
- `invoke()` calls shared dispatch, `invoke_hook()` calls built-in hook logic with typed input/output
- Use `assert-struct` crate for concise assertions on structured output (e.g., `assert_struct!(hook_out, { nudges: ["tokio"], .. })`)
- Write initial smoke tests for the unified dispatch (e.g., `invoke(["crate", "--list"])`, `invoke(["start"])`)
- From this point on, each subsequent step should include integration tests

### Step 4: Add toasty + sqlite dependencies

- Add `toasty` (with `sqlite` feature) to `Cargo.toml`
- Create `src/state.rs` module with the toasty models and DB setup
- DB init: create tables on first access, store at `~/.symposium/state.0.sqlite`
- All models defined here: `SkillActivation`, `SkillNudge`, `SessionState`, `WorkspaceCache`

### Step 5: Factor out workspace metadata module (`src/workspace.rs`)

- Extract `crate_sources::workspace_semver_pairs()` usage into a `workspace` module
- The module provides a `Workspace` struct that caches deps using the `WorkspaceCache` toasty model
- Cache key: `cwd` + `Cargo.lock` mtime. If mtime matches the DB record, return cached deps; otherwise re-scan and update. Different subdirectories of the same workspace get separate cache entries — acceptable tradeoff for simplicity.
- When refreshing (mtime changed), also load plugins, resolve skill groups, and upsert `AvailableSkill` rows (crate_name + skill_dir_path) into the DB. This is the shared "ensure DB is fresh" step that all hooks call on entry.
- Used by: `main.rs` (CLI `crate` command), `mcp.rs` (MCP tools), `hook.rs` (all hook events on entry), `start` (dynamic crate list)
- Replaces the ad-hoc `let workspace = crate_sources::workspace_semver_pairs(&cwd)` calls scattered across the codebase

### Step 6: Rework `symposium start` to include dynamic crate list

- `tutorial::render_cli()` and `tutorial::render_mcp()` currently return a static template
- Rename to `start` module. Change to: static Rust guidance (baked in) + dynamic output of `skills::list_output()`
- This means `start` needs to load plugins and scan the workspace, which it doesn't do today
- Both CLI and MCP paths use this via the shared dispatch
- Remove the `symposium rust` and `symposium tutorial` CLI subcommands — `symposium start` subsumes both. Rename `Commands::Tutorial` to `Commands::Start`. The `rust` name lives on only as the MCP tool name.

### Step 7: PostToolUse activation recording

- Add `PostToolUse` to the `HookEvent` enum
- Parse the PostToolUse payload: `tool_name`, `tool_input`, `tool_response`, `session_id`, `cwd`
- On entry, run the shared "ensure DB is fresh" step (Cargo.lock mtime check → refresh `WorkspaceCache` + upsert `AvailableSkill` rows if stale)
- Detect successful `symposium crate <name>` invocations (Bash tool with successful exit) and `rust` MCP tool calls with `["crate", "<name>"]` — record a `SkillActivation` using `session_id` from the hook payload. This is the primary activation path.
- Additionally, when Read or Bash successfully accessed a path matching an `AvailableSkill.skill_dir_path` in the DB, record a `SkillActivation` for that skill. This is a simple DB query — no plugin loading needed at this point.
- The `symposium crate` command itself does not write to the DB — it just returns content
- PreToolUse remains for plugin hook dispatch only (no built-in activation logic)

### Step 8: Add UserPromptSubmit hook handling

- Extend `HookEvent` enum with `UserPromptSubmit`
- Parse the new payload fields: `session_id`, `prompt`, `cwd`
- On entry, run the shared "ensure DB is fresh" step (same as PostToolUse)
- On prompt:
  1. Increment `SessionState.prompt_count` for this session
  2. Read available skills from DB (`AvailableSkill` rows for this `cwd`)
  3. Extract crate names mentioned in code-like contexts in the prompt (backtick-delimited, fenced code blocks, Rust paths — see Open Question 1)
  4. Query DB for activations and nudges in this session
  5. For each mentioned crate with a skill but no activation:
     - If **never nudged** → nudge, record `SkillNudge` with current prompt count
     - If **nudged but `prompt_count - nudge.at_prompt >= RENUDGE_THRESHOLD`** → nudge again, update `at_prompt`
     - Otherwise → skip (already nudged recently)
- Return `HookOutput` with `additionalContext` containing the nudge
- `nudge_interval` is configurable in `config.toml` (default **50** prompts). Set to **0** to disable nudges entirely (no `UserPromptSubmit` processing at all).

### Step 9: Update Claude Code plugin

- Update `hooks.json` to register `PostToolUse` and `UserPromptSubmit` hooks
- The hook commands invoke `symposium hook post-tool-use` and `symposium hook user-prompt-submit`
- Update the static skill file to say: "Critical. Invoke `symposium start` before authoring Rust."
- The bootstrap script (`symposium.sh`) passes through session_id env var

### Step 10: Update design documentation

- Update `md/design/current-status.md` with new hook events and state tracking
- Update `md/design/implementation-overview.md` with `state.rs` module
- Update `md/design/overview.md` if the architecture diagram changes

## Testing Strategy

### Unit tests

- **Crate mention detection** — pure string parsing, many edge cases (backticks, fenced blocks, false positives like "log"/"time")
- **Skill directory path matching** — given a tool target path and known skill directories, does it match?
- **Predicate parsing/matching** — already well-tested, extend as needed
- **State layer CRUD** — test against a real temp-dir SQLite database
- **Hook output structs** — built-in hook logic returns typed `HookOutput`, not raw JSON; test the logic, not stdout

### Integration tests (`tests/`)

Fixture-based integration tests that simulate agent workflows. Located in `tests/` as cargo integration tests.

**Composable fixtures** (`tests/fixtures/<name>/`): Fixtures are composable — each fixture is a directory fragment, and tests combine them by overlaying multiple fixtures into a single tempdir. This avoids duplicating shared fixture data.

Example fixture fragments:
- `workspace0/` — `Cargo.toml` + `Cargo.lock` + `src/lib.rs` (a workspace with tokio/serde deps)
- `plugins0/` — local plugin manifests + skill files (no network needed)

```
tests/fixtures/workspace0/
  Cargo.toml        # workspace with deps (tokio, serde, etc.)
  Cargo.lock        # checked in for mtime caching
  src/lib.rs        # can be empty

tests/fixtures/plugins0/
  plugins/          # local plugin manifests + skill files
```

**Test harness** (`tests/testlib/`):
```rust
testlib::with_fixture(["workspace0", "plugins0"], |test| {
    // `test` wraps a Symposium::from_dir(tempdir) with fixtures overlaid
    let output = test.invoke(["start"]);
    assert!(output.contains("tokio"));

    let hook_out = test.invoke_hook(UserPromptSubmit {
        session_id: "s1",
        prompt: "I need to use `tokio`",
        cwd: test.workspace_root(),
    });
    assert_struct!(hook_out, {
        additional_context: =~ r"tokio",
    });

    // After the PostToolUse hook records a successful activation, no more nudges
    test.invoke_hook(PostToolUse {
        tool_name: "Bash",
        tool_input: { "command": "symposium crate tokio" },
        tool_response: { "stdout": "...", "exit_code": 0 },
        session_id: "s1",
    });
    let hook_out = test.invoke_hook(UserPromptSubmit {
        session_id: "s1",
        prompt: "I need to use `tokio`",
        cwd: test.workspace_root(),
    });
    assert_struct!(hook_out, {
        additional_context: None,
    });
});
```

- `TestContext` creates a `Symposium::from_dir(tempdir)` — fully isolated, no env var manipulation, parallelizable
- `invoke()` calls the shared dispatch function (same code as CLI + MCP), returns the output string
- `invoke_hook()` calls the built-in hook logic, returns typed `HookOutput`
- Fixtures are self-contained (local plugins, no network); some tests may still hit crates.io for download-path testing

## Open Questions

1. **Crate mention detection** — ~~Simple substring match?~~ **Resolved:** Match crate names only in code-like contexts within the prompt text:
   - Inside inline code: `` `foo` ``, `` `foo::Bar` ``
   - Inside fenced code blocks: ``` ```...``` ```
   - As part of a Rust path: `foo::` or `::foo`

   This avoids false positives from natural language (e.g., "log" in "log in to the server", "time" in "it's time to"). Implementation: extract text from backtick-delimited and fenced regions, then check for crate name as a word boundary or path segment (`\bcrate_name\b` or `crate_name::`).

2. **Start becomes async** — ~~Is this acceptable?~~ **Resolved:** Yes, async is fine.

3. **Session ID plumbing for CLI** — ~~No session ID outside hooks.~~ **Resolved:** Skill activations are only recorded by the hook layer, which always has `session_id` from the hook payload. The `symposium crate` command itself does not write to the DB. No cwd fallback needed.

4. **Hook latency** — ~~Scanning workspace on every prompt is slow.~~ **Resolved:** Cache workspace deps in the DB keyed by `Cargo.lock` mtime. On each hook invocation, stat `Cargo.lock` — if mtime matches cached value, use cached deps. If changed (or missing), re-scan and update cache.

## FAQ

### Q: How do built-in hooks (PostToolUse, UserPromptSubmit) coexist with plugin hooks?

**Answer:** Both run. Hooks are "chained" — the built-in behavior is one link in the chain, and plugin hooks still fire as additional links. The exact chaining/output-merging semantics are still being defined upstream, but the design assumption is that built-in logic and plugin hooks coexist for the same event type. PreToolUse remains for plugin hook dispatch only.

### Q: Should the static skill reference MCP tool or CLI command?

**Answer:** The CLI command. The skill text says "run `symposium start`" — the agent invokes it via the shell.

### Q: What about `cargo add` via Bash changing Cargo.toml?

**Answer:** No special handling needed. `cargo add` also updates `Cargo.lock`, so the mtime-based cache invalidation in the `UserPromptSubmit` hook will catch it on the next prompt. There is no PostToolUse hook — workspace changes are detected entirely via `Cargo.lock` mtime.

### Q: Is renaming the MCP `rust` tool to `start` a clean break?

**Answer:** Moot — the tool stays named `rust`. It now takes a `Vec<String>` of args and dispatches like the CLI. No rename needed.

### Q: The Claude Code plugin currently has no `skills/` directory — is Step 8 creating from scratch?

**Answer:** Yes. The current plugin only has `hooks/`. The `skills/rust/SKILL.md` referenced in the implementation overview is generated by `just skill` but that infrastructure lives on the main branch (see `agent-plugins/claude-code/skills/` in main). This worktree will need to create the static skill file as part of Step 9.

### Q: Should `WorkspaceCache` be keyed by resolved workspace root instead of `cwd`?

**Answer:** No — just use `cwd` directly. Different subdirectories of the same workspace may produce duplicate cache entries, but this avoids the bootstrapping problem of needing `cargo_metadata` to find the workspace root (which is what we're trying to cache). The tradeoff is acceptable.

### Q: Is toasty ready for this use case?

**Answer:** Yes. Toasty's derive macro and SQLite backend are validated and ready for the simple CRUD needed here (skill activations, nudge tracking, session state, workspace cache).

### Q: Should Step 0 be done incrementally or all at once?

**Answer:** All at once. The codebase is small enough that threading `&Symposium` through every module in one pass is straightforward. No shim/bridge period needed. `Symposium` also owns logging init (replaces `config::init()`).

### Q: Should Step 3 (test infrastructure) move before Steps 1-2?

**Answer:** No. The integration test harness API (`test.invoke()`, `test.invoke_hook()`) depends on the unified dispatch (Step 1) and typed hook output (Step 2). Building the harness first would mean rewriting it after those refactors. Existing unit tests provide sufficient coverage during Steps 0-2.

### Q: What happens if the SQLite DB is locked, corrupt, or unwritable during a hook?

**Answer:** Propagate the error. A non-zero exit code from a hook is a non-blocking error in Claude Code (stderr shown in verbose mode, execution continues). No special swallowing or fallback needed.

### Q: Could `cwd` in hook payloads be a subdirectory rather than the workspace root?

**Answer:** Possibly — it's unclear whether Claude Code always sends the project root or the agent's current directory. If `cwd` varies, it causes cache misses and redundant re-scans, but that's acceptable — the re-scan is not expensive enough to warrant more complex workspace-root resolution.

### Q: Is `nudge-interval = 50` too high for typical sessions?

**Answer:** It's fine. One nudge is almost always sufficient — re-nudging is a safety net for very long sessions. The default is easy to change later if needed.

### Q: Will missing crate mentions outside code-like contexts be a problem?

**Answer:** No. Matching only inside backticks, fenced code blocks, and Rust paths avoids false positives from common English words ("log", "time", "rand"). If the agent mentions a crate in plain prose without backticks, the nudge arrives slightly later — when the crate inevitably appears in a code context. Acceptable tradeoff.

### Q: How does PostToolUse detect skill activation via file path without re-loading plugins?

**Answer:** At the start of every hook invocation, we run a shared "ensure DB is fresh" step: check `Cargo.lock` mtime against `WorkspaceCache` in the DB. If stale (or first run), re-scan workspace deps + load plugins + resolve skill directories → upsert `AvailableSkill` rows (crate_name, skill_dir_path) into the DB. If fresh, skip. Then PostToolUse path-matching is just a DB query against `AvailableSkill.skill_dir_path` — no plugin loading needed in the hot path.

### Q: Is the PostToolUse / UserPromptSubmit hook payload shape confirmed?

**Answer:** Yes, confirmed against [Claude Code hooks docs](https://code.claude.com/docs/en/hooks). All hooks receive `session_id`, `cwd`, `transcript_path`, `permission_mode`, and `hook_event_name` via stdin JSON. PostToolUse additionally provides `tool_name`, `tool_input`, `tool_response`, and `tool_use_id`. For Bash tools specifically, `tool_response` includes `exit_code`, `stdout`, and `stderr` as separate fields. UserPromptSubmit provides `prompt`. Hook output supports `hookSpecificOutput.additionalContext` (capped at 10,000 characters) and requires `hookEventName` to match the event.

## Implementation Notes (deviations from plan)

These are places where the actual implementation diverged from the plan above. Review these particularly.

### Structural: lib.rs split

Added `src/lib.rs` to re-export all modules as a library crate. This was not in the plan but was necessary because Rust integration tests (`tests/`) can only import library crates, not binary crates. All modules are `pub` in the lib. The binary (`src/main.rs`) imports from `symposium::*` instead of declaring `mod` directly.

### Step 0: `Arc<Symposium>` for MCP

The MCP server closures need owned/shared access to `Symposium`, so `main()` wraps it in `Arc<Symposium>`. The `mcp::serve` function takes `Arc<Symposium>` while all other call sites take `&Symposium`. This is a minor signature difference from the plan's uniform `&Symposium` threading.

### Step 1: CLI subcommands kept alongside dispatch

The plan said to remove the `symposium rust` and `symposium tutorial` CLI subcommands. In practice, both were kept: `Tutorial` as a static-only variant (no workspace scanning), and `Rust` forwarding its argument through dispatch. This preserves backwards compatibility. A new `Start` subcommand was added.

### Step 3: No `assert-struct` crate

The plan called for `assert-struct` for concise assertions on structured output. The implementation uses standard `assert!`/`assert_eq!` instead. Coverage is equivalent.

### Step 4: Toasty API differences from plan

The WIP's model definitions used `i64` for auto-increment IDs. Toasty actually requires `u64` for `#[auto]` fields. The `toasty::create!` macro doesn't support full `crate::path::Type` syntax — `use` imports are needed. `exec()`, `delete()`, and `update()` all take `&mut dyn Executor` and return builders needing `.exec(&mut db)`.

### Step 5: `Symposium` does not own the DB handle

The plan mentioned `Symposium` would later own the DB handle. In practice, the DB is opened fresh via `state::open_db(config_dir)` in each hook invocation. This is simpler and avoids lifetime issues with `Db` requiring `&mut` for all operations. SQLite connection pooling inside toasty makes this efficient enough.

### Step 8: SkillNudge update strategy

Toasty's `delete()` consumes `self` (moves), and toasty models don't derive `Clone`. The plan assumed we could delete + re-insert nudges. Instead, the implementation inserts a new nudge row with the current prompt count and uses `max_by_key(at_prompt)` to find the latest nudge per crate. Old nudge rows accumulate but are harmless.
