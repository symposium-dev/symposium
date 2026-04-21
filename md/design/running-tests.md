# Running tests

## Quick start

```bash
cargo test              # simulation + configured agents
```

By default, `cargo test` runs simulation tests and then re-runs agent-mode tests against each agent listed in `test-agents.toml`. On a fresh clone (no file), the defaults are `claude-sdk` and `kiro-cli-acp`.

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
| `claude-sdk` | Claude Agent SDK (Python) | Requires `uv` + `ANTHROPIC_API_KEY` |
| `kiro-cli-acp` | Kiro CLI via ACP | Requires `kiro-cli` in PATH |
| Any name from `acpr --list` | ACP registry via `acpr` | Auto-downloaded |

## Filtering to a single agent

Override with the `SYMPOSIUM_TEST_AGENT` env var:

```bash
SYMPOSIUM_TEST_AGENT=kiro-cli-acp cargo test --test hook_agent
```

This ignores `test-agents.toml` and runs only the specified agent.

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
