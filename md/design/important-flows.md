# Important flows

This section describes the logic of each `symposium` command.

- [Hook flow](./hook-flow.md) — how `symposium hook` dispatches events to plugin hooks and builtin handlers.
- [Sync workspace flow](./sync-workspace-flow.md) — how `symposium sync --workspace` scans dependencies and updates the project config.
- [Sync agent flow](./sync-agent-flow.md) — how `symposium sync --agent` installs extensions and registers hooks.
- [Init user flow](./init-user-flow.md) — how `symposium init --user` sets up user-wide configuration.
- [Init project flow](./init-project-flow.md) — how `symposium init --project` sets up project-level configuration.
- [Cargo fmt reminder flow](./cargo-fmt-reminder-flow.md) — how Symposium detects Rust file changes and reminds the agent to run `cargo fmt`.
