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

Show details for a specific plugin, including its TOML configuration and source file path.

### `cargo agents plugin validate`

```bash
cargo agents plugin validate <PATH> [--no-check-crates]
```

Validate a plugin source directory, a single plugin, [Agent Plugins package](./agent-plugins.md) or skill directory, or a single TOML manifest file. Useful when authoring plugins.

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a directory, a `plugin.json` (validated as its package directory), or a single `.toml` file |
| `--no-check-crates` | Skip checking that crate names in predicates exist on crates.io |

Each item is reported with its kind (`plugin`, `agent plugin` or `skill`). Warnings, such as a dormant plugin or an unknown `plugin.json` field, do not affect the exit code; an entry that fails to load, or a skill that does not meet the [skill format](./skill-definition.md), makes the command fail.
