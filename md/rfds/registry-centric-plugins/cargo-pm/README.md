# The `cargo` PM

## TL;DR

- The `cargo` PM bridges crates.io (and alternative Rust registries) to Symposium's plugin system.
- It is a separate binary (`symposium-pm-cargo`) communicating with Symposium via JSON-RPC over stdio.
- `resolve` takes an opaque TOML value using cargo's dependency format.
- `fetch` leverages the existing cargo toolchain to obtain crate sources.
- `list-deps` reads `Cargo.toml`/`Cargo.lock` to report direct workspace dependencies.
- Every crate is implicitly a plugin — no opt-in required.

## Motivation

Most Symposium users today are Rust developers. Their project dependencies live on crates.io. The cargo PM makes these dependencies discoverable as plugin sources — if `serde` ships skills, or if a recommendations entry references `serde`, the cargo PM is what connects the dots.

## Change in a nutshell

In the cargo PM, **every crate is a plugin**. No opt-in is required. A crate can optionally include a `Symposium.toml` at its root directory for explicit configuration — but if absent, an empty one is synthesized and [plugin defaults](../plugin-model/README.md) apply (which discovers `skills/` and `.agents/skills/` directories).

This means a crate author can ship skills by simply adding a `skills/` directory:

```
my-crate/
├── Cargo.toml
├── src/
│   └── lib.rs
└── skills/
    └── my-crate-usage/
        └── SKILL.md
```

No `Symposium.toml` needed. When a user depends on `my-crate`, the cargo PM's `list-deps` reports it, discovery finds the plugin content (via defaults), and the skills are offered for installation.

## Detailed plans

### Package-ids

The cargo PM defines package-ids as `(cargo, $crate-name, $version)`. For example: `(cargo, serde, 1.0.210)`, `(cargo, tokio, 1.38.0)`.

### `resolve` schema

Symposium passes the TOML value from `source.cargo = { ... }` to the cargo PM uninterpreted. The cargo PM accepts the same format cargo uses for dependency specifications — crate names as keys, version requirements as values:

```toml
[[plugins]]
source.cargo = { serde-skills = "1" }

[[plugins]]
source.cargo = { foo = "1.*", bar = "2.0" }
```

`resolve` queries the registry index and returns one package-id per resolved crate:

```
source.cargo = { serde-skills = "1.*" }
→ resolve → [(cargo, serde-skills, 1.2.3)]
```

### `search` behavior

`search` receives a package-id tuple (from another PM's `list-deps` result, passed during discovery). If the tuple's `pm` field is `cargo`, it searches the cargo registry for matching crates with Symposium plugin content.

**How we detect plugin content in a crate:**

1. **`Symposium.toml` at crate root** — explicit opt-in.
2. **Presence of `skills/` directory** — implicit. Convention-based discovery.
3. **Keyword convention** — crate authors add a `symposium-plugin` keyword. Search filters on this.

If the tuple's `pm` field is not `cargo`, return empty.

### `fetch` behavior

Given a package-id like `(cargo, serde-skills, 1.2.3)`:

1. Use the existing cargo toolchain to obtain crate sources — leveraging `~/.cargo/registry/src/` (the unpacked source cache) or triggering `cargo fetch` if needed.
2. Locate the unpacked crate source in cargo's cache.
3. The crate root directory is the plugin directory (defaults apply to discover skills, etc.).
4. Copy (or symlink) the plugin root into the destination path provided by Symposium.

This approach ensures compatibility with users who have custom registry configurations, alternative registries, or corporate mirrors — we go through cargo rather than around it.

### `list-deps` behavior

Reads the workspace to report direct Rust dependencies.

**Input:** workspace root directory (where `Cargo.toml` lives).

**Strategy:**

1. If `Cargo.lock` exists, read it — it has exact versions for all resolved dependencies. Return direct dependencies (those listed in workspace members' `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`).
2. If no lockfile, fall back to reading `Cargo.toml` manifests for dependency names (without exact versions).

**Output:** set of package-id tuples, e.g., `[(cargo, serde, 1.0.210), (cargo, tokio, 1.38.0)]`.

**Workspace handling:**
- For a workspace with multiple members, union all members' direct dependencies.
- Path dependencies within the workspace are excluded (those are the user's own crates, not external deps).
- Dev-dependencies are included (they're still dependencies the user works with).

**Performance:**
- Parse `Cargo.lock` directly (it's a TOML file). No `cargo metadata` invocation.
- Cache results keyed on `Cargo.lock` mtime.
- If `Cargo.lock` hasn't changed, return cached results immediately.

### Chained plugins for independent release

If a crate author wants to release plugin content on a separate schedule from their library, they add a `Symposium.toml` to their crate with a chained plugin:

```toml
# In widget-lib's Symposium.toml
[[plugins]]
source.cargo = { widget-symposium = "1" }
```

This tells Symposium: "when this plugin is loaded, also load `widget-symposium`." The chained plugin can be published and updated independently.

### Alternative registries

The cargo PM defaults to crates.io but can be configured to use alternative registries. Configuration mechanism TBD — likely via cargo's own registry configuration in `~/.cargo/config.toml`, which the cargo PM inherits naturally since it uses the cargo toolchain.

## Frequently asked questions

### Why keys in `source.cargo` rather than `name`/`version` fields?

The key-value style (`{ foo = "1.0", bar = "2.0" }`) mirrors how `[dependencies]` works in `Cargo.toml`, which is familiar to Rust users. It also naturally supports multiple crates per entry.

### How does `search` know which crates have plugin content without downloading them all?

Three approaches, in order of preference:
1. **Keyword convention** — crate authors add a `symposium-plugin` keyword. Search filters on this.
2. **Registry metadata** — if crates.io exposes enough metadata to detect `Symposium.toml` or `skills/` presence.
3. **Recommendations fallback** — for crates found via recommendations, we already know they have content.

### Why not use `[package.metadata.symposium]` in Cargo.toml?

We use `Symposium.toml` as the single configuration mechanism across all ecosystems. This avoids splitting plugin configuration between ecosystem-specific manifest files and keeps things consistent — whether your plugin comes from cargo, npm, or git, the configuration lives in `Symposium.toml`.

## Implementation plan and status

### Step 1: `list-deps` from Cargo.lock

Parse `Cargo.lock` directly for dependency names and versions. Handle workspace members, exclude path deps. Mtime-based caching.

- [ ] PR: cargo PM `list-deps`

### Step 2: `resolve` with registry index

Query the crates.io index (or alternative registry) to resolve version requirements to exact versions.

- [ ] PR: cargo PM `resolve`

### Step 3: `fetch` via cargo toolchain

Leverage cargo's registry cache to locate crate sources. Copy to dest.

- [ ] PR: cargo PM `fetch`

### Step 4: `search` with plugin detection

Search the registry, filter for plugin content (via keyword or metadata), rank results.

- [ ] PR: cargo PM `search`
