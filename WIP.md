# Agent Integration Tests

## Goal

Rework existing integration tests so they can run in two modes:

1. **Simulation** (default, CI): call `execute_hook` directly with synthetic payloads (what we do today)
2. **Agent** (manual, env-gated): drive a real agent that triggers symposium hooks naturally

First target agent: **Claude Code** via the Claude Agent SDK.

## Context

- Zulip thread: https://symposium-dev.zulipchat.com/#narrow/channel/530148-general/topic/Agent.20integration.20tests/with/585850598
- Current tests call `TestContext::invoke_hook()` → `hook::execute_hook()` with hand-crafted JSON payloads
- No real agent is ever in the loop
- The old `hook_nudge.rs` and `hook_activation.rs` tests have been removed on main;
  remaining `invoke_hook` usages are in `init_sync.rs` and `skill_sync_options.rs`
  (testing init/sync side-effects, not hook behavior directly)
- New hook-focused tests will use the `submit` abstraction below

## Design

### Env variable

`SYMPOSIUM_TEST_AGENT` controls the mode:
- Unset / empty → simulation (default)
- `claude` → run with Claude Code via Agent SDK

### Central `submit` method

Tests describe scenarios in agent-neutral terms:

```rust
let result = ctx.submit(
    "I need to use `serde`",       // prompt (used in agent mode only)
    &[
        HookStep::session_start(),
        HookStep::user_prompt("I need to use `serde`"),
    ],
).await;

// Assert on collected hook outputs (canonical symposium form)
assert!(result.has_context_containing("serde"));
```

The prompt is only used in agent mode — simulation mode ignores it and
iterates the steps directly. This is intentional: the prompt tells the real
agent what to do, while the steps are the precise simulation equivalent.

`HookStep` is agent-neutral:

```rust
enum HookStep {
    SessionStart,
    UserPromptSubmit { prompt: String },
    PreToolUse { tool_name: String, tool_input: Value },
    PostToolUse { tool_name: String, tool_input: Value, tool_response: Value },
}
```

`submit` automatically injects:
- `cwd` from `ctx.workspace_root` (or `ctx.sym.config_dir()` as fallback)
- `session_id` as `"test-session-id"` (fixed value)

**In simulation mode:** `submit` iterates the steps, converts each `HookStep`
→ `symposium::InputEvent` → agent wire format → `execute_hook()`, collects
outputs. This exercises the full serialize→parse round-trip, which is the
whole point — testing the exact sequence that occurs at runtime.

**In agent mode:** `submit` ignores the explicit steps, sends the prompt to the
real agent, and collects whatever hooks actually fire via the trace (see below).

Both modes produce a `SubmitResult`.

`execute_hook` and `invoke_hook` remain available for precise hard-coded tests
that don't need the dual-mode abstraction.

### `SubmitResult`

```rust
struct SubmitResult {
    /// Hook invocations in order, with both input and output in canonical form.
    hooks: Vec<HookTrace>,
}

struct HookTrace {
    event: HookEvent,
    input: serde_json::Value,
    output: serde_json::Value,
}
```

Convenience methods added as tests need them, e.g.:
- `result.outputs_for(HookEvent::UserPromptSubmit)` — filter by event
- `result.has_context_containing("serde")` — substring match on additionalContext

Two paths to the same result:
- **Simulation**: each `execute_hook` call produces an input/output pair → collect
- **Agent**: read the `SYMPOSIUM_HOOK_TRACE` JSONL file → parse into `Vec<HookTrace>`

### Hook tracing

In agent mode, hooks are invoked as separate processes
(`symposium hook claude <event>` shelled out by the agent). We need a way to
observe what happened.

**Approach: `SYMPOSIUM_HOOK_TRACE`** — when this env var is set to a file path,
the CLI's `hook::run()` function appends a JSONL entry for each hook invocation:

```jsonl
{"event":"SessionStart","agent":"claude","input":{...},"output":{...}}
{"event":"UserPromptSubmit","agent":"claude","input":{...},"output":{...}}
```

**Concurrency safety:** The trace file path is not set as a process-wide env var
in the test process. Instead, `submit` passes the path to the Python harness as
a CLI argument, and the harness sets `SYMPOSIUM_HOOK_TRACE` in the environment
of the agent subprocess it spawns. Each test gets its own trace file via its
own tempdir.

### Project-local test environment

The existing test infrastructure already handles this well:

1. `with_fixture()` creates an isolated temp dir with config + workspace
2. `ctx.symposium(&["init", "--project"])` registers hooks in
   `.claude/settings.json` (or equivalent for other agents)
3. Hook commands are `symposium hook claude <event>` — they find project
   config from cwd

**Binary path issue:** Hook commands use bare `symposium`, but in tests we need
the cargo-built binary. Solution: prepend the cargo target dir to `$PATH` in
the agent subprocess environment. The Python harness inherits this modified PATH.

### Claude Agent SDK harness

A Python script (`tests/agent_harness/run_scenario.py`) invoked via `uv run`:

```
uv run --with claude-agent-sdk \
    tests/agent_harness/run_scenario.py \
    --prompt "..." \
    --cwd /tmp/test-xxx \
    --trace /tmp/test-xxx/hook-trace.jsonl
```

The script:
- Starts a Claude Agent SDK session with `setting_sources=["project"]`
- Uses `allowed_tools` and `max_turns` to keep runs focused
- Returns when the agent completes

No venv or requirements.txt needed — just `uv` on PATH.

Test prompts will be very mechanical (e.g., "Please edit the file `foo` and
write `bar`") to keep agent behavior deterministic and cheap.

## Prerequisites (agent mode)

- `uv` installed
- `ANTHROPIC_API_KEY` set in environment
- `SYMPOSIUM_TEST_AGENT=claude` set in environment

## Running tests

### Simulation mode (default, CI)

```bash
# Run all tests (simulation mode — no agent needed)
cargo test

# Run only the hook agent tests
cargo test --test hook_agent
```

Simulation mode is the default. It calls `execute_hook` directly with
synthetic payloads, exercising the full serialize→parse round-trip.

### Agent mode (manual, requires API key)

```bash
# Prerequisites: uv installed, ANTHROPIC_API_KEY set
SYMPOSIUM_TEST_AGENT=claude cargo test --test hook_agent
```

Agent mode drives a real Claude Code session via the Claude Agent SDK.
Each test sends a prompt to the agent, which triggers symposium hooks
naturally. The test reads the JSONL trace file to verify hook behavior.

Agent-mode tests are more expensive (API calls) and slower, so they're
gated behind the `SYMPOSIUM_TEST_AGENT` env var. They're intended for
manual validation, not CI.

## Open Questions

- [ ] Do we need `#[ignore]` on agent tests, or is the env variable check sufficient?
  - Leaning toward env variable only — `submit` early-returns simulation results
    if agent not configured
- [ ] Should agent-mode assertions be strictly looser than simulation-mode?
  - Probably yes — agent might trigger extra hooks, order may vary
  - `SubmitResult` should support both exact and "contains" style assertions

## Key code locations

- `src/hook.rs` — `execute_hook()` is the core hook pipeline (parse → builtin →
  plugins → serialize). `run()` is the CLI entry point that reads stdin, calls
  `execute_hook`, writes stdout. The JSONL tracing goes in `run()`.
- `symposium-testlib/src/lib.rs` — `TestContext`, `with_fixture()`, `invoke_hook()`.
  The new `submit`, `HookStep`, `SubmitResult` types go here.
- `src/agents.rs` — `register_claude_hooks()` writes hook commands to
  `.claude/settings.json`. The commands are bare `symposium hook claude <event>`.
- `src/hook_schema/` — per-agent modules (claude.rs, codex.rs, etc.) that handle
  the agent wire format ↔ symposium canonical type conversions.
- `tests/init_sync.rs`, `tests/skill_sync_options.rs` — the remaining tests that
  use `invoke_hook`. These test init/sync side-effects, not hook behavior.
- `symposium-testlib/build.rs` — sets `SYMPOSIUM_FIXTURES_DIR` so tests can find
  `tests/fixtures/`.

## FAQ / Design rationale

**Why does simulation mode round-trip through the agent wire format?**
We considered calling `dispatch_builtin` + `dispatch_plugin_hooks` directly on
symposium canonical types, which would be simpler. But the whole point is testing
the exact code path that runs at runtime: agent sends JSON → symposium parses it
→ processes → serializes response. The parse/serialize layer has had bugs before.

**Why is the prompt a separate argument from the steps?**
The prompt is only used in agent mode (it's what gets sent to the real agent).
Simulation mode ignores it and uses the steps. We considered extracting the prompt
from `HookStep::UserPromptSubmit`, but that couples the two modes unnecessarily —
the agent-mode prompt might be phrased differently from the simulation payload
(e.g., "Please edit foo" vs a precise PostToolUse payload).

**Why a Python harness and not a Rust-native SDK client?**
The Claude Agent SDK is only available in Python and TypeScript. Since we need to
drive a real Claude Code session (with hooks, tool execution, etc.), we shell out
to a Python script. `uv run --with claude-agent-sdk` makes this zero-setup.

**Why `SYMPOSIUM_HOOK_TRACE` as an env var read by the CLI, not something else?**
In agent mode, the agent shells out to `symposium hook claude <event>` as a
separate process for each hook invocation. We can't instrument these in-memory.
The env var is the simplest way to get the CLI to log its own hook I/O. The
alternative (wrapping the binary) is more fragile.

**Why not set `SYMPOSIUM_HOOK_TRACE` as a process-wide env var in the test?**
Cargo runs tests in parallel. If two agent-mode tests set the env var
simultaneously, child processes could see the wrong value. Instead, `submit`
passes the trace path to the Python harness as a CLI arg, and the harness sets
the env var only in the agent subprocess it spawns.

**Why bare `symposium` in hook commands, and how do tests handle it?**
`register_claude_hooks()` writes `symposium hook claude <event>` (no absolute
path) — this is correct for production (binary is on PATH). For tests, we
prepend the cargo target dir to `$PATH` in the agent subprocess environment so
the freshly-built binary is found. The `CARGO_BIN_EXE_symposium` env var
(set by cargo during `cargo test`) gives us the path.

**What happened to hook_nudge.rs and hook_activation.rs?**
They were removed on main. The nudge/activation logic may come back in a
different form. The `submit` abstraction is designed for writing new hook-focused
tests going forward, not primarily for converting old ones.

**Why `session_id: "test-session-id"` (fixed)?**
Session state is keyed by session_id on disk. A fixed value keeps things simple
and deterministic. If a test needs multiple sessions, it can call `submit` with
different steps or use `invoke_hook` directly.

## Tasks

- [x] Add `SYMPOSIUM_HOOK_TRACE` support to `hook::run()` (JSONL append)
- [x] Define `HookStep`, `HookTrace`, and `SubmitResult` types in testlib
- [x] Implement `submit` — simulation mode (iterate steps, call `execute_hook`)
- [x] Create Python harness script (`tests/agent_harness/run_scenario.py`)
- [x] Implement `submit` — agent mode (shell out via `uv run`, read trace)
- [x] Write a new dual-mode test as proof of concept
- [x] Document how to run agent tests locally
- [x] Add cargo alias for convenience
