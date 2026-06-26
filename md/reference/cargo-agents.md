# The `cargo agents` command

`cargo agents` manages agent extensions for Rust projects. It discovers skills based on your project's dependencies and configures your AI agent to use them.

## Subcommands

| Command | Description |
|---------|-------------|
| [`cargo agents init`](./cargo-agents-init.md) | Set up user-wide configuration |
| [`cargo agents sync`](./cargo-agents-sync.md) | Synchronize skills with workspace dependencies |
| [`cargo agents use`](./cargo-agents-use.md) | Add a plugin crate |
| [`cargo agents remove`](./cargo-agents-remove.md) | Remove a plugin crate |
| [`cargo agents status`](./cargo-agents-status.md) | Show crates in use and active plugins |
| [`cargo agents plugin`](./cargo-agents-plugin.md) | Manage and validate plugins |
| [`cargo agents self-update`](./cargo-agents-self-update.md) | Update symposium to the latest version |
| [`cargo agents crate-info`](./cargo-agents-crate-info.md) | Find crate sources (agent-facing) |

## Global options

| Flag | Description |
|------|-------------|
| `-v`, `--verbose` | Print detailed decision trace (which plugins matched, which skills were considered, etc.) |
| `--json` | Output structured JSON report to stdout; suppresses human-readable output. Combine with `-v` to include the full decision trace. |
| `-q`, `--quiet` | Suppress status output |
| `--help` | Print help |
| `--version` | Print version |

The `-v` and `--json` flags work with `sync`, `status`, and `plugin validate`. During hook dispatch, decision events are emitted at debug level and appear in verbose output when testing hooks.
