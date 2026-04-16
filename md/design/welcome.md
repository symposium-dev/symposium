# Contributing to Symposium

Welcome! This section is for people who want to work on Symposium itself. If you're a crate author who wants to publish skills or hooks for your library, see [Supporting your crate](../crate-authors/supporting-your-crate.md) instead.

## Building and testing

Symposium is a standard Cargo project with both a library and a binary:

```bash
cargo check              # type-check
cargo test               # run the test suite
cargo run -- start       # run locally: Rust guidance + crate skill list
cargo run -- crate tokio # run locally: crate-specific guidance
```

Tests use snapshot assertions via the `expect-test` crate. If a snapshot changes, run with `UPDATE_EXPECT=1` to update it:

```bash
UPDATE_EXPECT=1 cargo test
```

## Logging and debugging

Symposium uses `tracing` for structured logging. Each invocation writes a timestamped log file to `~/.symposium/logs/`.

The default log level is `info`. To get more detail, set the level in `~/.symposium/config.toml`:

```toml
[logging]
level = "debug"   # or "trace" for maximum detail
```

Log files are named `cargo-agents-YYYYMMDD-HHMMSS.log`. When debugging an issue, the log file from the relevant invocation is usually the best place to start.

## What to read next

- [Key repositories](./repositories.md) — the repos that make up Symposium
- [Key modules](./module-structure.md) — the main pieces of the codebase
- [Important flows](./important-flows.md) — key paths through the code
- [Integration test harness](./test-harness.md) — how to write and run tests
- [Governance](./governance.md) — how we work together
