# Auto-discovery from dependencies

When you add a crate to your workspace dependencies, Symposium can automatically discover and install its plugins — no explicit `cargo agents install` required. This is controlled by the **dependency allow list**.

## How it works

1. You add `serde` to your `Cargo.toml` dependencies.
2. On the next sync, Symposium checks the allow list and sees that `serde` is approved.
3. Symposium fetches the `serde` crate source and scans it for `SYMPOSIUM.toml` or `skills/`.
4. Any discovered skills are installed into your agent.

From the user's perspective, it just works — add a dependency, get skills.

## The allow list

Not every workspace dependency is auto-discovered. A crate must appear in the **dependency allow list** before Symposium will scan it. This prevents unwanted plugins from appearing (e.g., from typosquatting crates) and keeps sync fast.

The allow list is populated from two places:

### 1. Installed plugin crates (e.g., `symposium-recommendations`)

Plugin crates can declare which workspace deps are approved for auto-discovery:

```toml
# Inside symposium-recommendations/serde/SYMPOSIUM.toml
dependency-allow-list.crates = ["serde", "serde_json"]
```

The default `symposium-recommendations` crate ships with an allow list covering popular crates.

### 2. Your own config

You can add entries directly in `~/.symposium/config.toml`:

```toml
# Approve specific crates
dependency-allow-list = ["my-internal-crate", "another-crate"]
```

Or opt in to full auto-discovery:

```toml
# Trust any workspace dep that contains a SYMPOSIUM.toml
dependency-allow-list = ["*"]
```

## What gets installed

Once a crate is activated (whether by explicit install or auto-discovery), the same rules apply. Symposium scans for `SYMPOSIUM.toml` files (each defines a plugin with skills, hooks, MCP servers, etc.) and falls back to scanning `skills/` recursively if no manifest is found. Predicates on skills are evaluated as normal — active skills get installed, inactive ones don't.

## Relationship to explicit install

Auto-discovery and explicit install end up in the same place — the crate gets scanned as a plugin source. The difference is just how it got there:

| | Explicit install | Auto-discovery |
|---|---|---|
| Trigger | `cargo agents install foo` | `foo` in workspace deps + allow list |
| Persisted in config | Yes (`[[installed-crate]]`) | No (re-evaluated each sync) |
| Version | Specified at installation (`@1`, `@=1.2.3`) | Matches workspace dep version |
| Updates | Automatically when new compatible versions are released | When you update your deps  |
