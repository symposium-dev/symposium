# Installing plugins manually

Symposium plugins are distributed as crates. Install one with:

```bash
cargo agents install my-plugin
```

This fetches the crate, scans it for plugins and skills, and adds it to your configuration. Skills are synced into your agents on the next `cargo agents sync` (or automatically if `auto-sync` is enabled).

## Version tracking

By default, Symposium tracks the latest version and upgrades automatically. You can control this with Cargo-style version requirements:

```bash
cargo agents install my-plugin@1       # track within 1.x
cargo agents install my-plugin@1.2     # track within ^1.2
cargo agents install my-plugin@=1.2.3  # pin to exact version, never upgrade
```

## Other sources

You can install from git repositories or local paths:

```bash
# From a git repository
cargo agents install --git https://github.com/my-org/my-plugin

# From a local directory (changes picked up immediately)
cargo agents install --path ./my-local-plugin
```

## Uninstalling

```bash
cargo agents uninstall my-plugin
```

Skills and hooks contributed by the crate are cleaned up on the next sync.

## Seeing what's installed

```bash
cargo agents status
```

This shows all installed crates, which plugins/skills are active in the current workspace, and which are inactive (with reasons).

## What happens inside a plugin crate

When you install a crate, Symposium scans it from the root for `SYMPOSIUM.toml` files. Each one defines a plugin (with skills, hooks, MCP servers, etc.). Additionally, unless overridden by a `SYMPOSIUM.toml` at the crate root, Symposium also looks for skills in `skills/` and installs them directly.

This means the simplest plugin crate is just a crate with a `skills/` directory — no manifest needed.
