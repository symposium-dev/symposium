# Auto-discovery from dependencies

When a workspace dependency is approved by discovery policy, Symposium can scan that dependency as a plugin source without requiring an explicit `cargo agents install`.

## How it works

1. You add a crate such as `serde` to your `Cargo.toml` dependencies.
2. During sync, Symposium builds discovery candidates from workspace dependencies.
3. It evaluates `[discovery.allow]` and `[discovery.deny]` rules from user config and already-loaded plugins.
4. Allowed candidates resolve through the crate registry and are scanned with the same source-root discovery rules as installed sources.

The default policy is deny-all. A specific rule beats a wildcard rule; if allow and deny have the same specificity, deny wins.

## Plugin-provided policy

Curated plugin crates can approve dependency candidates:

```toml
# Inside symposium-recommendations/serde/SYMPOSIUM.toml
[discovery.allow]
crates = { serde = "*", serde_json = "*" }
```

## User policy

Users can add policy directly in `~/.symposium/config.toml`:

```toml
[discovery.allow]
crates = { my-internal-crate = "*", another-crate = "*" }
```

Or opt in to all crate dependency candidates:

```toml
[discovery.allow]
crates = "*"
```

To allow every registry candidate type, use the scalar shorthand:

```toml
[discovery]
allow = "*"

[discovery.deny]
crates = { abandoned-crate = "*" }
```

## What gets installed

Once a crate is activated by discovery, it is just another resolved source root. Symposium loads `SYMPOSIUM.toml` at the root or synthesizes an empty manifest, applies default `skills/`, workspace-gated `.agents/skills/`, and nested-manifest search rules, then evaluates predicates normally.

## Relationship to explicit install

Auto-discovery and explicit install both feed the resolved source graph:

| | Explicit install | Auto-discovery |
|---|---|---|
| Trigger | `cargo agents install foo` | Workspace dependency + discovery allow rule |
| Persisted in config | Yes (`[installed.crates]`, `installed.paths`, or `installed.git`) | No; re-evaluated each sync |
| Version | User-specified dependency requirement | Workspace-resolved dependency version |
| Provenance | `installed()` | `dependency()` |
