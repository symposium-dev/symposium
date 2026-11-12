# The `symposium` command

`symposium` manages agent extensions for Rust projects. It discovers skills based on your project's dependencies and configures your AI agent to use them.

`cargo agents` is the same command, installed as a cargo subcommand: `cargo agents <args>` accepts the same subcommands and options as `symposium <args>` and behaves the same way. Help and usage messages name the one you ran.

## Subcommands

| Command | Description |
|---------|-------------|
| [`symposium init`](./symposium-init.md) | Set up user-wide configuration |
| [`symposium sync`](./symposium-sync.md) | Synchronize skills with workspace dependencies |
| [`symposium search`](./symposium-search.md) | Search configured registries for plugins |
| [`symposium use`](./symposium-use.md) | Enable a plugin by name (`--remove` to disable) |
| [`symposium status`](./symposium-status.md) | Show which plugins are enabled for this workspace, and why |
| [`symposium plugin`](./symposium-plugin.md) | Manage plugin sources |
| [`symposium self-update`](./symposium-self-update.md) | Update symposium to the latest version |
| [`symposium crate-info`](./symposium-crate-info.md) | Find crate sources (agent-facing) |

## Global options

| Flag | Description |
|------|-------------|
| `-v`, `--verbose` | Print detailed decision trace (which plugins matched, which skills were considered, etc.) |
| `--json` | Output structured JSON report to stdout; suppresses human-readable output. Combine with `-v` to include the full decision trace. |
| `--update <LEVEL>` | Plugin source update behavior: `none` (default), `check`, `fetch` |
| `-q`, `--quiet` | Suppress status output |
| `--help` | Print help |
| `--version` | Print version |

The `-v` and `--json` flags work with `sync`, `plugin list`, and `plugin validate`. During hook dispatch, decision events are emitted at debug level and appear in verbose output when testing hooks.
