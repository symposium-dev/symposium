# `cargo agents uninstall`

Uninstall a plugin crate.

## Usage

```bash
cargo agents uninstall <CRATE>
```

## Behavior

Removes the specified crate from `~/.symposium/config.toml`. Any skills, hooks, or MCP servers contributed by the crate are cleaned up on the next `cargo agents sync`.

## Examples

```bash
cargo agents uninstall my-plugin
```

## See also

- [`cargo agents install`](./cargo-agents-install.md) — install a plugin crate
- [`cargo agents status`](./cargo-agents-status.md) — see what's installed and active
