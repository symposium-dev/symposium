# `cargo agents plugin`

Manage plugin sources.

## Usage

```bash
cargo agents plugin <SUBCOMMAND>
```

## Subcommands

### `cargo agents plugin sync`

```bash
cargo agents plugin sync [PROVIDER]
```

Fetch or update git-based plugin sources. If a provider name is given, syncs only that provider (ignoring `auto-update` settings). If omitted, syncs all providers that have `auto-update = true`.

### `cargo agents plugin list`

```bash
cargo agents plugin list
```

List all configured plugin sources and the plugins they provide.

### `cargo agents plugin show`

```bash
cargo agents plugin show <PLUGIN>
```

Show a plugin: its id and its effective configuration as TOML, after validation has filled in defaults and promoted inline installations.

### `cargo agents plugin validate`

```bash
cargo agents plugin validate <PATH> [--no-check-crates]
```

Validate plugins without installing anything. Useful when authoring plugins. Each item is reported with its kind: `plugin`, `agent plugin` or `skill`. `<PATH>` is one of:

- A crate: a directory whose `Cargo.toml` has a `[package]` table, or that `Cargo.toml` or the crate's `Symposium.toml`. The crate is validated as its users get it: `[package.metadata.symposium]` and `Symposium.toml` merged, named after the crate, with the default `skills/` group. A malformed `[package.metadata.symposium]` or `Symposium.toml`, which loading skips with only a log entry, is an error here, and so is a crate with no plugin content.
- A plugin or skill directory, or an [Agent Plugins package](./agent-plugins.md) directory or its `plugin.json`, validated as that one entry.
- A [plugin source](./plugin-source.md) directory, such as a checkout of the recommendations repository: every entry in it.
- Any other `.toml` file, validated as a registry plugin manifest.

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a directory or a manifest file |
| `--no-check-crates` | Skip checking that crate names in predicates exist on crates.io |

Besides errors, it warns about a plugin that loads but does not work the same on every agent:

- a plugin name containing `.`, which Antigravity rejects;
- an Agent Plugins package with no `version`, which agents that cache plugins by version, such as Codex, may not refresh;
- a hook event handled only by agent-format hooks, which fire on that agent alone (see [Agent-specific hooks](../crate-authors/authoring-a-plugin.md#agent-specific-hooks));
- a skill file or folder that is not installed: a link that resolves outside its package, a broken link, or a link back into the skill.

### `cargo agents plugin import`

```bash
cargo agents plugin import <PATH> [--out <DIR>]
```

Turn a Claude Code plugin into an [Agent Plugins package](./agent-plugins.md) that Symposium loads, written to `<DIR>/<name>/`. The command refuses to overwrite an existing folder, and validates the result as [`plugin validate`](#cargo-agents-plugin-validate) does.

| Flag | Description |
|------|-------------|
| `<PATH>` | The Claude Code plugin folder, the one holding `.claude-plugin/plugin.json` |
| `--out <DIR>` | Where to write the package. Defaults to the current directory |

What each part of the Claude Code plugin becomes:

| Claude Code | Package |
|-------------|---------|
| `.claude-plugin/plugin.json` | `plugin.json`, keeping `name`, `version`, `description`, `author`, `homepage`, `repository`, `license` and `keywords`. The name is rewritten to follow the [name rule](./agent-plugins.md#names). Without the file, the folder name is used. |
| `skills/` | Copied unchanged |
| `commands/*.md` | A skill each, `skills/<command>/SKILL.md`, named after the command. A command with no `description`, or whose name an existing skill already has, is left out. |
| `hooks/hooks.json` | `[[hooks]]` entries with `format = "claude"` in a `Symposium.toml`, each running its original command through `sh -c` |
| `.mcp.json` | `[[mcp_servers]]` entries in the same `Symposium.toml` |
| Other files | Copied unchanged, so the scripts a hook runs come along |

A `Symposium.toml` is written only when there are hooks or MCP servers. It then also declares `[[skills]] source.path = "skills"`, since a `Symposium.toml` beside `plugin.json` gets no default skill group.

Symposium runs a `format = "claude"` hook only in Claude Code, with `CLAUDE_PLUGIN_ROOT` set to the package folder, so the hook works as it did in the plugin. To run a hook in the other agents, add a [`format = "symposium"`](./plugin-definition.md#agent-specific-hooks) hook next to it.

Everything that is not imported is reported, one line each:

- `agents/`, `.lsp.json`, and `plugin.json` fields outside the list above, such as `userConfig`. The package format has no place for them.
- Hooks for an event Symposium does not handle, hooks whose `type` is not `command`, and hooks using a field other than `command` and `timeout`, such as `if` or `async`. `timeout` itself is dropped.
- MCP servers that use `${CLAUDE_PLUGIN_ROOT}`, which agents do not expand outside a Claude Code plugin.

A skill that mentions `${CLAUDE_PLUGIN_ROOT}` is copied, with a warning: Claude Code only fills that in for the skills of its own plugins.

The imported plugin names no crate, so in a plugin source it stays dormant until enabled with [`cargo agents use`](./cargo-agents-use.md).

### `cargo agents plugin try`

```bash
cargo agents plugin try <PATH>
cargo agents plugin try --remove <PATH or NAME>
```

Install a plugin you are writing in your agents, the way a user would have it, so you can open an agent and use it. `<PATH>` is a plugin directory (with a `Symposium.toml`), an [Agent Plugins package](./agent-plugins.md) directory (with a `plugin.json`) or a skill directory (with a `SKILL.md`).

The plugin is linked into `~/.symposium/plugins/` under its name and enabled in every workspace, then the current workspace is synced. Symposium reads your directory in place, so an edit reaches your agents on the next sync: `cargo agents sync`, or the automatic one when an agent starts a session. Most agents then need a new session (Claude Code also takes `/reload-plugins`, Copilot CLI `/restart`).

A plugin with its own `depends-on` or predicates still follows them: it is active only in workspaces where they hold.

After syncing, `try` asks each configured agent what it loaded, using the agent's own listing command (none of them calls a model), and reports the plugin's skills it does not list:

| Agent | Command |
|-------|---------|
| Codex CLI | `codex debug prompt-input hi` |
| GitHub Copilot | `copilot skill list --json` |
| Goose | `goose skills list` |
| OpenCode | `opencode debug skill` |

Claude Code, Antigravity CLI and Kiro have no such command: start a new session in the project and run `/skills`. An agent whose command is not on `PATH` is skipped.

`--remove` discards the trial: it removes the link and the enablement, and syncs, which removes everything installed for the plugin in the current workspace. Other workspaces are cleaned on their next sync. A directory in `~/.symposium/plugins/` that `try` did not link is left alone.

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to the plugin or skill directory; with `--remove`, its path or the plugin name |
| `--remove` | Discard the trial |
