# Installing plugins manually

Symposium plugins are distributed as crates. Add one with:

```bash
cargo agents use my-plugin
```

This fetches the crate, scans it for plugins and skills, and adds it to your configuration. Skills are synced into your agents on the next `cargo agents sync` (or automatically if `auto-sync` is enabled).

## Version tracking

By default, Symposium tracks the latest version and upgrades automatically. You can control this with Cargo-style version requirements:

```bash
cargo agents use my-plugin@1       # track within 1.x
cargo agents use my-plugin@1.2     # track within ^1.2
cargo agents use my-plugin@=1.2.3  # pin to exact version, never upgrade
```

## Other sources

You can add plugins from git repositories or local paths:

```bash
# From a git repository
cargo agents use --git https://github.com/my-org/my-plugin

# From a local directory (changes picked up immediately)
cargo agents use --path ./my-local-plugin
```

## Removing

```bash
cargo agents remove my-plugin
```

Skills and hooks contributed by the crate are cleaned up on the next sync.

## Seeing what's in use

```bash
cargo agents status
```

This shows all crates in use, which plugins/skills are active in the current workspace, and which are inactive (with reasons).

## What happens inside a plugin crate

When you add a crate, Symposium scans it from the root for `SYMPOSIUM.toml` files. Each one defines a plugin (with skills, hooks, MCP servers, etc.). Nested manifests are discovered automatically (each becomes its own independent plugin).

If no `SYMPOSIUM.toml` is found anywhere, Symposium falls back to scanning `skills/` recursively for `SKILL.md` files. This means the simplest plugin crate is just a crate with a `skills/` directory — no manifest needed.
