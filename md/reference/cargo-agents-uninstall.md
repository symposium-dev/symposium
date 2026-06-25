# `cargo agents uninstall`

Uninstall a plugin source from user config.

## Usage

```bash
cargo agents uninstall <CRATE> ...
cargo agents uninstall --path <PATH> ...
cargo agents uninstall --git <URL> ...
```

## Behavior

Removes matching entries from `~/.symposium/config.toml`: crate-registry
entries from `[installed.crates]`, direct path sources from `installed.paths`,
and direct git sources from `installed.git`.

The current implementation is config-only. Once the resolved-source sync path
is wired in, skills, hooks, and MCP servers contributed by the removed source
will be cleaned up on the next `cargo agents sync`.

## Examples

```bash
cargo agents uninstall my-plugin
cargo agents uninstall --path ./my-local-plugins
cargo agents uninstall --git https://github.com/my-org/my-plugins
```

## See also

- [`cargo agents install`](./cargo-agents-install.md) — install a plugin source
- [`cargo agents status`](./cargo-agents-status.md) — see what's installed and active
