# Integration test harness

Integration tests live in `tests/` and use the `symposium-testlib` crate for composable, isolated test environments.

## Writing a test

### 1. Pick your fixtures

Look at `tests/fixtures/` and see if an existing fixture covers your scenario. Fixtures are composable — you can layer multiple together:

```rust
// Just config + plugins
let ctx = with_fixture(&["plugins0"]);

// Config + plugins + a Cargo workspace with serde/tokio deps
let ctx = with_fixture(&["plugins0", "workspace0"]);

// A plugin that captures stdin to $TEST_DIR/captured.json in claude format
let ctx = with_fixture(&["capture-claude0"]);
```

If nothing fits, create a new directory under `tests/fixtures/`. Use `dot-symposium/` for config and plugin files. The harness discovers `config.toml` and `Cargo.toml` by filename.

### 2. Write the test

A minimal test that sends a hook event and checks the output:

```rust
use symposium::hook::HookEvent;
use symposium::hook_schema::HookAgent;
use symposium_testlib::with_fixture;

#[tokio::test]
async fn my_test() {
    let ctx = with_fixture(&["plugins0"]);

    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::PreToolUse,
            &serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "ls"},
                "cwd": "/tmp",
            }),
        )
        .await
        .unwrap();

    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    // assert on v...
}
```

### 3. Available operations

`TestContext` provides:

- **`invoke_hook(agent, event, &payload)`** — the full hook pipeline (`parse → builtin → plugins → serialize`). Same code path as `symposium hook <agent> <event>`. The payload is any `impl Serialize` matching the agent's wire format. Returns agent wire-format output bytes.

- **`invoke(&["crate", "--list"])`** — runs a dispatch command and returns the output string.

- **`symposium(&["sync", "--agent"])`** — runs a CLI command against the test context.

- **`normalize_paths(&output)`** — replaces temp directory paths with `$CONFIG_DIR` for stable snapshots.

### 4. Snapshot testing

Use `expect-test` for inline snapshots:

```rust
use expect_test::expect;

expect![[r#"
    ...expected output...
"#]].assert_eq(&serde_json::to_string_pretty(&v).unwrap());
```

Run `UPDATE_EXPECT=1 cargo test` to auto-fill or update snapshots.

## Fixtures

Browse the full set at [`tests/fixtures/`](https://github.com/symposium-dev/symposium/tree/main/tests/fixtures). A few examples:

- **`plugins0/`** — config + a local plugin with a serde skill and session-start context. The baseline for most tests.
- **`capture-claude0/`** — a plugin that captures stdin to `$TEST_DIR/captured.json` in Claude wire format. There's one per agent, plus `capture-symposium0/` for canonical format.

Fixtures compose: `with_fixture(&["plugins0", "workspace0"])` layers both into one tempdir.

### Variable expansion

Text files (`.toml`, `.md`, `.json`, `.txt`, `.ts`, `.js`) have variables expanded when copied:

- **`$TEST_DIR`** — the tempdir root. Use for paths that resolve at test time (e.g., `command = "cat > $TEST_DIR/captured.json"`).
- **`$BINARY`** — path to the `symposium` binary (`CARGO_BIN_EXE_symposium`).
