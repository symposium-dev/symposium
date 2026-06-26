# `cargo agents use`

Add a plugin crate.

## Usage

```bash
cargo agents use [OPTIONS] <CRATE>[@<VERSION>] ...
cargo agents use [OPTIONS] --path <PATH> ...
cargo agents use [OPTIONS] --git <URL> ...
```

## Behavior

Adds the specified source to `~/.symposium/config.toml`: crate-registry forms
are stored under `[used.crates]`, direct path sources under
`used.paths`, and direct git sources under `used.git`. Source
resolution and discovery then scan the resolved source tree for plugins/skills.

### Source types

- **crates.io** (default) — Specify just a crate name, optionally with a version requirement.
- **git** — Use `--git <URL>` to add from a git repository.
- **path** — Use `--path <PATH>` to add from a local directory (useful for development).

### Version requirements

Uses Cargo's version requirement semantics (the `^` operator is implicit):

| Form | Meaning |
|------|---------|
| `cargo agents use foo` | Always tracks latest (no version constraint) |
| `cargo agents use foo@1` | Tracks within 1.x (i.e., `^1`) |
| `cargo agents use foo@1.2` | Tracks within ^1.2 |
| `cargo agents use foo@1.2.3` | Tracks within ^1.2.3 |
| `cargo agents use foo@=1.2.3` | Exact pin, never upgrades |

Running `cargo agents use foo@1.3` on an already-added crate updates its version constraint.

## Examples

```bash
# Add from crates.io (latest)
cargo agents use symposium-tokio

# Add with version constraint
cargo agents use symposium-tokio@1

# Pin to exact version
cargo agents use symposium-tokio@=1.2.3

# Add from git
cargo agents use --git https://github.com/my-org/my-plugins

# Add from local path (for development)
cargo agents use --path ./my-local-plugins
```

## See also

- [`cargo agents remove`](./cargo-agents-remove.md) — remove a plugin source
- [`cargo agents status`](./cargo-agents-status.md) — see what's active
- [Configuration](./configuration.md#used) — the config file format
