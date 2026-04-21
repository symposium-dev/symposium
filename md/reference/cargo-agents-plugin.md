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

Validate a plugin source directory or a single TOML manifest file. Useful when authoring plugins.

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a directory or a single `.toml` file |
| `--no-check-crates` | Skip checking that crate names in predicates exist on crates.io |
