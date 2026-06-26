# `cargo agents remove`

Remove a plugin source from user config.

## Usage

```bash
cargo agents remove <CRATE> ...
cargo agents remove --path <PATH> ...
cargo agents remove --git <URL> ...
```

## Behavior

Removes matching entries from `~/.symposium/config.toml`: crate-registry
entries from `[used.crates]`, direct path sources from `used.paths`,
and direct git sources from `used.git`.

The current implementation is config-only. Once the resolved-source sync path
is wired in, skills, hooks, and MCP servers contributed by the removed source
will be cleaned up on the next `cargo agents sync`.

## Examples

```bash
cargo agents remove my-plugin
cargo agents remove --path ./my-local-plugins
cargo agents remove --git https://github.com/my-org/my-plugins
```

## See also

- [`cargo agents use`](./cargo-agents-use.md) — add a plugin source
- [`cargo agents status`](./cargo-agents-status.md) — see what's active
