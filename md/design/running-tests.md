# Running tests

## Quick start

```bash
cargo test                                        # simulation only
SYMPOSIUM_ENABLE_AGENT_TESTING=1 cargo test       # simulation + configured agents
```

In PowerShell, set the variable first. It lasts for the rest of the session, so remove it to go back to simulation only:

```powershell
$env:SYMPOSIUM_ENABLE_AGENT_TESTING = "1"
cargo test
Remove-Item Env:SYMPOSIUM_ENABLE_AGENT_TESTING
```

Agent-mode tests run only when `SYMPOSIUM_ENABLE_AGENT_TESTING` is set. Without it, a `TestMode::AgentOnly` test runs nothing and still reports `ok`; with `--nocapture` it prints that no agent ran. When it is set, `cargo test` runs simulation tests and then re-runs agent-mode tests against each agent listed in `test-agents.toml`. On a fresh clone (no file), the default is `claude-sdk`.

## Configuring test agents

Create `test-agents.toml` in the repo root (gitignored):

```toml
# Run against these agents. Use `acpr --list` to see ACP registry agents.
test-agents = ["claude-sdk"]
```

Set to `[]` to skip agent tests entirely (used in CI):

```toml
test-agents = []
```

Available agent names:

| Name | Backend | Notes |
|------|---------|-------|
| `claude-sdk` | Claude Agent SDK (Python) | Requires `uv`, and `ANTHROPIC_API_KEY` or a logged-in `claude` |
| `kiro-cli-acp` | Kiro CLI via ACP | Requires `kiro-cli` in PATH |
| Any name from `acpr --list` | ACP registry via `acpr` | Auto-downloaded |

## Filtering to a single agent

Override with the `SYMPOSIUM_TEST_AGENT` env var:

```bash
SYMPOSIUM_ENABLE_AGENT_TESTING=1 SYMPOSIUM_TEST_AGENT=kiro-cli-acp cargo test --test hook_agent
```

```powershell
$env:SYMPOSIUM_ENABLE_AGENT_TESTING = "1"
$env:SYMPOSIUM_TEST_AGENT = "kiro-cli-acp"
cargo test --test hook_agent
```

This ignores `test-agents.toml` and runs only the specified agent. `cargo test-agent` is an alias for `cargo test --test hook_agent --`, so it needs the same variables.

For example, to reproduce #344 (a skill installed by the first `SessionStart` is not loadable on the first prompt) on Claude Code:

```bash
SYMPOSIUM_ENABLE_AGENT_TESTING=1 SYMPOSIUM_TEST_AGENT=claude-sdk cargo test --test hook_agent skill_installed_by_session_start -- --nocapture
```

```powershell
$env:SYMPOSIUM_ENABLE_AGENT_TESTING = "1"
$env:SYMPOSIUM_TEST_AGENT = "claude-sdk"
cargo test --test hook_agent skill_installed_by_session_start -- --nocapture
```

## Running specific test files

```bash
cargo test --test hook_agent       # just the agent integration tests
cargo test --test init_sync        # just the init/sync tests
cargo test --test dispatch         # just the CLI dispatch tests
```

## Debugging test failures

Add `--nocapture` to see test output (agent messages, hook traces):

```bash
cargo test --test hook_agent -- --nocapture
```

On failure, the test's temporary directory is preserved and its path is printed to stderr so you can inspect the fixture state.

## CI

CI runs the suite with `cargo nextest run --workspace`, which runs every test in its own process and the test binaries in parallel, and runs doctests separately with `cargo test --workspace --doc`. A test that relies on state another test in the same binary left behind can pass under `cargo test` and fail in CI; reproduce it locally with `cargo nextest run`.

## Windows

CI runs the full test suite on `windows-latest` as part of the `test` matrix (see `.github/workflows/ci.yml`). To run the tests locally on Windows:

- Install Git for Windows and make sure `sh` is on `PATH`. Git ships it at `C:\Program Files\Git\usr\bin`. Several tests spawn `sh` to run script-based hooks and predicates, so a missing `sh` shows up as unrelated-looking hook failures.
- The repo's `.gitattributes` normalizes checked-out text files to LF. This keeps shebang'd fixtures and shell scripts runnable regardless of `core.autocrlf`.

The self-update tests run on Windows: `set_mock_cargo` runs the `#!/bin/sh` mock through `sh` via a one-line `.cmd` shim (production spawns the cargo override directly, so no production code changes). The two `auto_update_re_execs_*` tests are `#[ignore]`d on Windows (`#[cfg_attr(windows, ignore)]`): they overwrite the running binary with a shebang stand-in and re-exec into it, which needs Windows-native process replacement. They still compile on Windows, so they are skipped (not compiled out) and their helpers need no `#[cfg]`. Native Windows equivalents are described below.

`tests/self_update_windows.rs` uses real Cargo to install the dependency-free package in `tests/fixtures/self-update` into a temporary installation root. The fixture's `cargo.cmd` stubs registry version discovery and redirects installation to that local package; it does not implement Cargo's file replacement. A negative control first proves that Cargo cannot replace a running image, then retries after that process exits. The updater tests cover successful installation for both CLI executable names, rollback after a Cargo installation failure, and automatic command resumption with arguments, hook stdin, and exit status preserved. All subprocess waits have deadlines and use Windows jobs that terminate the process tree on cancellation. Paths contain spaces, Cargo runs offline with an isolated home, and the user's installation is untouched.

Focused unit tests in `self_update/windows.rs` separately cover staging before mutation, denied source reads, activation failure after the first rename, rollback after partial replacement, preservation of the backup when rollback fails, manual recovery at the interruption boundary, and old-image cleanup. The replacement application's source is a normal Cargo fixture, rather than Rust generated and compiled by the test harness. The Unix shebang tests remain useful for Unix `exec`.
