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
