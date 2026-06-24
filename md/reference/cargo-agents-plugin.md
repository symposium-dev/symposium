# `cargo agents plugin`

Manage and validate plugins.

## Usage

```bash
cargo agents plugin <SUBCOMMAND>
```

## Subcommands

### `cargo agents plugin list`

```bash
cargo agents plugin list
```

List all installed plugin crates and the plugins they provide.

### `cargo agents plugin show`

```bash
cargo agents plugin show <PLUGIN>
```

Show details for a specific plugin, including its TOML configuration and source crate.

### `cargo agents plugin validate`

```bash
cargo agents plugin validate <PATH> [--no-check-crates]
```

Validate a plugin manifest or a directory containing plugins. Useful when authoring plugins.

| Flag | Description |
|------|-------------|
| `<PATH>` | Path to a `SYMPOSIUM.toml` file or a directory to scan |
| `--no-check-crates` | Skip checking that crate names in predicates exist on crates.io |
