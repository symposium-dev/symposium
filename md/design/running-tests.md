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

## Pi extension

The Pi bridge also has TypeScript and Node tests:

```bash
cd src/agents/pi
npm ci --ignore-scripts
npm run check
npm test
npm run test:integration
```

Use Node.js 22.19 or later. These tests do not call a model. `npm test` checks
event mapping, generated skill loading, and delivery of hook context with queued
inputs using Pi's session and extension runner.

`npm run test:integration` builds the native debug `cargo-agents` binary, then
checks actual CLI sync against Pi's project resource discovery and cancellation
of a running plugin command. Fixtures use temporary homes and local plugins;
they do not install packages or contact a model. The binary lookup honors Cargo's
target directory, or an explicit `SYMPOSIUM_TEST_BINARY` when invoking Node tests
directly. CI runs both suites on Linux, macOS, and Windows. Cancellation tests
exercise Unix signals and are explicitly skipped on Windows.

## Windows

CI runs the full test suite on `windows-latest` as part of the `test` matrix (see `.github/workflows/ci.yml`). To run the tests locally on Windows:

- Install Git for Windows and make sure `sh` is on `PATH`. Git ships it at `C:\Program Files\Git\usr\bin`. Several tests spawn `sh` to run script-based hooks and predicates, so a missing `sh` shows up as unrelated-looking hook failures.
- The repo's `.gitattributes` normalizes checked-out text files to LF. This keeps shebang'd fixtures and shell scripts runnable regardless of `core.autocrlf`.

The self-update tests run on Windows: `set_mock_cargo` runs the `#!/bin/sh` mock through `sh` via a one-line `.cmd` shim (production spawns the cargo override directly, so no production code changes). The two `auto_update_re_execs_*` tests are `#[ignore]`d on Windows (`#[cfg_attr(windows, ignore)]`): they overwrite the running binary with a shebang stand-in and re-exec into it, which needs Windows-native process replacement. They still compile on Windows, so they are skipped (not compiled out) and their helpers need no `#[cfg]`. Porting them to run on Windows is a tracked follow-up.
