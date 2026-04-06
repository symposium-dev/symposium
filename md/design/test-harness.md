# Integration test harness

Integration tests live in `tests/` and use the `symposium-testlib` crate for composable, isolated test environments.

## Fixtures

Test fixtures are directories under `tests/fixtures/` that provide fragments of a Symposium environment:

- **`plugins0/`** — a `dot-symposium/` directory containing a `config.toml` and a local plugin with a serde skill. No network access needed.
- **`workspace0/`** — a minimal `Cargo.toml` workspace with tokio and serde dependencies.

Fixtures are designed to compose. `with_fixture(&["plugins0", "workspace0"])` copies both into a single temp directory, giving you a complete environment with config, plugins, and a workspace.

## `TestContext`

`with_fixture()` returns a `TestContext` wrapping an isolated `Symposium` instance:

```rust
let ctx = with_fixture(&["plugins0", "workspace0"]);
```

The harness scans the copied files for `config.toml` (becomes the Symposium config directory) and `Cargo.toml` (becomes the workspace root). It panics if multiple of either are found.

`TestContext` provides three methods:

- **`invoke(&["start"])`** — parses args via clap (same as the MCP server would) and routes through the shared dispatch layer. Returns the output string.
- **`invoke_hook(payload)`** — calls the built-in hook logic directly with a typed payload (e.g., `PostToolUsePayload`, `PreToolUsePayload`). Returns a `HookOutput`.
- **`normalize_paths(&output)`** — replaces temp directory paths with `$CONFIG_DIR` so snapshots are stable across runs.

## Snapshot testing

Tests use the `expect-test` crate for inline snapshot assertions:

```rust
#[tokio::test]
async fn start() {
    let ctx = with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["start"]).await.unwrap();
    let output = ctx.normalize_paths(&output);
    expect![[r#"
        ...expected output...
    "#]].assert_eq(&output);
}
```

When output changes, run with `UPDATE_EXPECT=1` to update the inline snapshots in place:

```bash
UPDATE_EXPECT=1 cargo test
```

## Adding a new fixture

Create a directory under `tests/fixtures/` with whatever files your test needs. Convention: use `dot-symposium/` for config/plugin files (the harness discovers `config.toml` by filename, not by directory name). Compose it with existing fixtures via `with_fixture(&["your-fixture", "workspace0"])`.
