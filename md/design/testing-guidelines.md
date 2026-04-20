# Writing tests

Symposium tests run in two modes:

* **Simulation mode** — hooks and CLI calls are invoked directly by the harness. No real agent needed.
* **Agent mode** — a real agent session processes prompts and we verify it triggers the expected hooks.

Tests declare which mode they need via `TestMode`:

* `TestMode::SimulationOnly` — runs once in simulation.
* `TestMode::AgentOnly` — runs once per configured test agent.
* `TestMode::Any` — runs once in simulation + once per configured agent.

## 1. Create your setup by composing fixtures

Wrap your test in `with_fixture`, specifying the mode and fixtures:

```rust
use symposium_testlib::{TestMode, with_fixture};

#[tokio::test]
async fn my_test() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        // test body
        Ok(())
    }).await.unwrap();
}
```

Fixtures are directories under `tests/fixtures/`. They are overlaid into a tempdir:

```rust
with_fixture(TestMode::SimulationOnly, &["plugins0", "workspace0"], async |mut ctx| { ... })
```

`with_fixture` scans fixtures for `config.toml` (user config dir) and `Cargo.toml` (workspace root). In agent mode it automatically runs `init --add-agent` and `sync`.

For `TestMode::Any` and `TestMode::AgentOnly`, the test closure runs once per configured agent.

### Variable expansion in fixtures

Text files have variables expanded when copied:

- `$TEST_DIR` — the tempdir root.
- `$BINARY` — path to the `cargo-agents` binary.

### Fixture requirements

All fixture `config.toml` files must include `hook-scope = "project"` so that hooks are installed into the project directory rather than globally.

## 2. Write the test body

### Bimodal tests (`TestMode::Any`)

Use `ctx.prompt_or_hook` which dispatches based on mode:

```rust
with_fixture(TestMode::Any, &["plugins0", "project-plugins0"], async |mut ctx| {
    let result = ctx
        .prompt_or_hook("Say hello", &[HookStep::session_start()], HookAgent::Claude)
        .await?;

    assert!(!result.hooks.is_empty());
    assert!(result.has_context_containing("symposium start"));
    Ok(())
}).await.unwrap();
```

In agent mode, `prompt_or_hook` also asserts that the expected hook events appear in the trace.

### Agent-only tests (`TestMode::AgentOnly`)

Use `ctx.prompt` to send prompts to the real agent:

```rust
with_fixture(TestMode::AgentOnly, &["plugin-tokio-weather0", "workspace-empty0"], async |mut ctx| {
    ctx.prompt("Run `cargo add tokio` please!").await?;
    let result = ctx.prompt("Use the tokio-weather skill to answer: ...").await?;
    assert!(result.response.unwrap().contains("MAGIC SENTENCE"));
    Ok(())
}).await.unwrap();
```

### Simulation-only tests (`TestMode::SimulationOnly`)

Use `ctx.symposium` to invoke the CLI directly:

```rust
with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
    ctx.symposium(&["init", "--add-agent", "claude"]).await?;
    ctx.symposium(&["sync"]).await?;
    // assert on files...
    Ok(())
}).await.unwrap();
```
