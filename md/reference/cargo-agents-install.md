# `cargo agents install`

Install a plugin crate.

## Usage

```bash
cargo agents install [OPTIONS] <CRATE>[@<VERSION>] ...
cargo agents install [OPTIONS] --path <PATH> ...
cargo agents install [OPTIONS] --git <URL> ...
```

## Behavior

Adds the specified crate to `~/.symposium/config.toml` as an `[[installed-crate]]` entry, fetches the crate source, and discovers plugins/skills from it.

### Source types

- **crates.io** (default) — Specify just a crate name, optionally with a version requirement.
- **git** — Use `--git <URL>` to install from a git repository.
- **path** — Use `--path <PATH>` to install from a local directory (useful for development).

### Version requirements

Uses Cargo's version requirement semantics (the `^` operator is implicit):

| Form | Meaning |
|------|---------|
| `cargo agents install foo` | Always tracks latest (no version constraint) |
| `cargo agents install foo@1` | Tracks within 1.x (i.e., `^1`) |
| `cargo agents install foo@1.2` | Tracks within ^1.2 |
| `cargo agents install foo@1.2.3` | Tracks within ^1.2.3 |
| `cargo agents install foo@=1.2.3` | Exact pin, never upgrades |

Running `cargo agents install foo@1.3` on an already-installed crate updates its version constraint.

## Examples

```bash
# Install from crates.io (latest)
cargo agents install symposium-tokio

# Install with version constraint
cargo agents install symposium-tokio@1

# Pin to exact version
cargo agents install symposium-tokio@=1.2.3

# Install from git
cargo agents install --git https://github.com/my-org/my-plugins

# Install from local path (for development)
cargo agents install --path ./my-local-plugins
```

## See also

- [`cargo agents uninstall`](./cargo-agents-uninstall.md) — remove an installed crate
- [`cargo agents status`](./cargo-agents-status.md) — see what's installed and active
- [Configuration](./configuration.md#installed-crate) — the config file format
