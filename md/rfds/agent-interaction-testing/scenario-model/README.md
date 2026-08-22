# Scenario model

## Test frontends and shared engine

Agent interaction tests extend `symposium-testlib`; they do not create a second fixture or assertion system. Ordinary tests and `cargo xtask agent-test` use the same fixture composition, scenario model, event vocabulary, and assertions.

`cargo test` remains the frontend for deterministic and selected host scenarios. `cargo xtask agent-test` orchestrates environment selection, credentials, containers, filtering, real-agent execution, and artifact retention. A scenario does not change meaning when selected through a different frontend.

## Scenario registration and body

Each scenario has declarative registration metadata and an imperative Rust body. The runner can list and preflight the metadata without executing the body. The metadata declares:

- a stable name and short description;
- fixture layers and controlled services;
- required environment and agent capabilities;
- a capability witness for every real-agent interaction;
- deadlines, resource limits, and scenario-owned agent budgets;
- a least-privilege permission policy;
- contract rule identifiers; and
- artifact-retention policy.

The body is an ordinary asynchronous Rust function returning `Result`. It receives a constrained `ScenarioContext` for running commands, driving the terminal, querying an agent, mutating controlled fixture state, and making assertions. Using Rust control flow and `?` keeps a failure at the operation that caused it instead of reporting one interpreter failure for the whole journey.

```rust,ignore
async fn dependency_consent_accept(cx: &mut ScenarioContext) -> Result<()> {
    cx.run_init().await?;
    let mut sync = cx.spawn_sync_pty().await?;
    sync.wait_for("Enable this dependency?").await?;
    sync.press_enter().await?;
    cx.assert_skill_installed("fixture-skill")?;
    Ok(())
}
```

The body cannot access undeclared host paths, process-global environment, credentials, or agent-specific APIs. Those remain behind `ScenarioContext`, environment backends, and agent adapters. A paid query, external endpoint, fixture service, or privileged operation must be declared in metadata. The context rejects an operation that was not authorized by the registration metadata.

Scenarios select capabilities, not agent brands. Agent-specific paths, authentication fields, event types, and witness mechanisms remain in adapters.

Agent token budgets are cumulative across every provider request made for the journey, not merely the number of user-visible turns. They bound input, cache-read, cache-write, and output tokens separately, plus provider requests and tool calls. Cached tokens still count toward the token budget even when their dollar price is lower.

## State and branching

Each scenario follows one expected behavioral path. Accept, decline, ask-later, and Escape are separate scenarios that may reuse fixture descriptions but never writable state.

Every scenario and retry begins with a fresh workspace, user configuration, agent configuration, cache, services, and agent query context. Steps within one scenario share state deliberately, including across declared process restarts. Persistence and cache scenarios express repeated commands in that one journey because preserved state is what they test.

Scenario logic does not branch around unexpected output. A missing or different checkpoint fails at that step.

## Production process boundary

Authoritative user journeys invoke the compiled `cargo-agents` executable through the production-facing `cargo agents` command. Interactive commands run under a PTY. The runner captures the rendered terminal, sanitized raw bytes, exit status, structured events, and resulting state.

The runner must not silently replace a requested process or PTY step with an in-process call. Existing deterministic tests may continue calling Symposium's Rust entry points directly, but that path does not prove Cargo dispatch, PATH setup, terminal interaction, hook subprocesses, or process exit behavior.

## PTY scripting

The PTY driver parses ANSI output into a rendered screen instead of treating the stream as plain stdout. Waits query narrow anchors in that screen. Input steps represent lines and explicit keys such as Enter, Escape, arrows, EOF, and interrupt.

Ordinary scenarios use a fixed terminal size, UTF-8 locale, declared TERM and color mode, and the native PTY backend for the operating system, including ConPTY or equivalent on Windows. The terminal profile and backend are recorded with the result.

Screen normalization handles cursor movement, redraws, color, and newline differences. Raw sanitized bytes remain diagnostic evidence. A small rendering suite separately tests color and resizing; ordinary journeys do not snapshot the complete screen.

## Time-dependent scenarios

Tests never synchronize with fixed sleeps. Every wait targets an observable condition and has a real monotonic deadline.

This RFD does not add a production clock seam. Time-dependent tests mutate controlled persisted inputs before starting the command that observes them. Predicate-cache tests set the persisted expiry into the past, update-throttle tests set `state.toml`, filesystem tests set the relevant mtime, and sync tests may use `sync-debounce-secs = 0`.

These mutations test the production comparison against the real wall clock without waiting for time to pass. If a later contract cannot be tested this way, its clock abstraction requires a separate design. Container, agent, TLS, provider, and process deadlines always use real time.

## Metadata and body boundary

Preflight needs declarative metadata before fixtures, containers, credentials, or paid agents are started. Journey execution benefits from ordinary Rust: compiler-assisted refactoring, direct reuse of test helpers, native asynchronous control flow, and line-local errors through `?`.

Only registration metadata and the resulting execution plan are serializable. Scenario bodies are compiled Rust and are not loaded from TOML, YAML, or another scenario language. The constrained context preserves backend and adapter neutrality without exposing backend objects to the body. The [root rationale](../README.md#describe-complete-journeys-as-data) records why the design does not use a fully data-driven scenario format.
