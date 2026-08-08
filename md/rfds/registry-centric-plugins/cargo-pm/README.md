# The `cargo` PM

## TL;DR

- The `cargo` PM bridges crates.io (and alternative Rust registries) to Symposium's plugin system.
- It is a separate binary (`symposium-pm-cargo`) communicating with Symposium via JSON-RPC over stdio.
- `load_plugin` takes a crate name and version requirement in cargo's format.
- `fetch` leverages the existing cargo toolchain to obtain crate sources.
- `list_deps` reports direct workspace dependencies.
- Every crate is implicitly a plugin — no opt-in required.

## Motivation

Most Symposium users today are Rust developers. Their project dependencies live on crates.io. The cargo PM makes these dependencies discoverable as plugin sources — if `serde` ships skills, or if a recommendations entry references `serde`, the cargo PM is what connects the dots.

## Change in a nutshell

In the cargo PM, **every crate is a plugin**. No opt-in is required. A crate can optionally include a `Symposium.toml` at its root directory for explicit configuration, but if absent, an empty one is synthesized and [plugin defaults](../plugin-model/README.md) apply (which discovers `skills/` and `.agents/skills/` directories).

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

No `Symposium.toml` needed. When a user depends on `my-crate`, the cargo PM's `list_deps` reports it, discovery finds the plugin content (via defaults), and the skills are offered for installation.

## Detailed plans

### Package-ids

The cargo PM defines package-ids as `(cargo, $crate-name, $version)`. For example: `(cargo, serde, 1.0.210)`, `(cargo, tokio, 1.38.0)`.

### Chained-reference schema

A `[[plugins]]` chained reference names one crate, as a dependency atom or a table:

```toml
[[plugins]]
source.cargo = "serde-skills>=1"

[[plugins]]
source.cargo = { name = "serde-skills", version = "1.*" }
```

Symposium lowers either spelling to a package-id whose version component is the requirement, and sends it to `load_plugin`. The cargo PM resolves the requirement and answers with the exact version.

### `search` behavior

`search` receives a partial query string and searches crates.io by name, returning candidate crates.

The results are *candidates*, not confirmed plugin carriers: because every crate is implicitly a plugin, whether a given crate contributes anything is only known once it is fetched. This is deliberate: it lets `cargo agents use <crate>` name a crate the workspace doesn't depend on, and defers the question to the fetch/load step.

### `fetch` behavior

Given a package-id like `(cargo, serde-skills, 1.2.3)`:

1. A path dependency resolves to its local directory directly.
2. A `(name, version)` already unpacked resolves to that directory with no work at all. A published version is immutable, so once its source is on disk there is nothing to re-check and no reason to ask the network.
3. Otherwise use the existing cargo toolchain: `~/.cargo/registry/src/` (the unpacked source cache), falling back to a crates.io download.
4. The crate root directory is the plugin directory (defaults apply to discover skills, etc.).
5. Return that directory in place.

Step 2 is what makes `fetch` cheap enough to sit on the hook path. `list_deps`
caching keyed on `Cargo.lock` avoids re-resolving the graph; this avoids
re-acquiring the sources that resolution named. Only an unresolved version
requirement needs the registry, and only to turn it into an exact version.

This approach ensures compatibility with users who have custom registry configurations, alternative registries, or corporate mirrors — we go through cargo rather than around it.

### `list_deps` behavior

Reads the workspace to report direct Rust dependencies.

**Input:** the workspace root, supplied once at `initialize`.

**Output:** set of package-id tuples, e.g., `[(cargo, serde, 1.0.210), (cargo, tokio, 1.38.0)]`.

**Workspace handling:**
- For a workspace with multiple members, union all members' direct dependencies.
- Dev-dependencies are included (they're still dependencies the user works with).

**Performance:**
- Cache results on disk, keyed on `Cargo.lock` mtime.
- If `Cargo.lock` hasn't changed, return cached results immediately: no resolution at all.

### Workspace information

Symposium itself needs the workspace root and the member directories: for workspace-local plugins, for scoping `use` entries, and for locating agent skill directories. It reads them off the cargo resolver today.

Moving the cargo PM out of process means these cross the boundary, either as an extra method or as part of the `initialize` response. Loading plugins *from* those directories should stay in Symposium: they are local directory reads, and the workspace is a trust root whose policy core owns. The cargo PM's job is to report where the workspace is, not what it contains.

### Chained plugins for independent release

If a crate author wants to release plugin content on a separate schedule from their library, they add a `Symposium.toml` to their crate with a chained plugin:

```toml
# In widget-lib's Symposium.toml
[[plugins]]
source.cargo = "widget-symposium>=1"
```

This tells Symposium: "when this plugin is loaded, also load `widget-symposium`." The chained plugin can be published and updated independently.

### Alternative registries

The cargo PM defaults to crates.io but can be configured to use alternative registries. Configuration mechanism TBD — likely via cargo's own registry configuration in `~/.cargo/config.toml`, which the cargo PM inherits naturally since it uses the cargo toolchain.

## Frequently asked questions

### How does `search` know which crates have plugin content without downloading them all?

It doesn't, and doesn't try. Every crate is implicitly a plugin, so "has plugin content" is not knowable from the registry index: search returns name matches and the load step decides what each contributes.

A keyword convention such as `symposium-plugin` is deliberately not used as a filter: it would only distinguish anything once crate authors adopted it, and until then it would hide plugin-bearing crates that had not.

### When should a crate use `[package.metadata.symposium]` rather than a `Symposium.toml`?

Both work, and a crate may use both: the table is the same manifest schema,
embedded, and the two are merged with the file taking precedence. The table
suits a crate declaring a small amount of plugin configuration that does not
justify another file. A crate with real plugin content should ship a
`Symposium.toml`, where the configuration is easier to find and to read.

Note this is the same capability the PM interface generalizes. Reading plugin configuration out of an ecosystem's own manifest is exactly what [returning a synthesized manifest](../pm-interface/README.md#what-crosses-the-wire) is for; `[package.metadata.symposium]` is that idea applied to cargo, and an npm PM would do the same with `package.json`.

## Implementation plan and status

Steps here follow the [PM interface plan](../pm-interface/README.md#implementation-plan-and-status): the cargo PM binary is step 4 there, and cannot start before the protocol exists.

### Step 1: Extract the cargo PM into a standalone library

Separate workspace resolution, crate fetching, and crate-manifest merging from Symposium's core, so the binary is a thin wrapper. Keeping it a library is also what lets unit tests keep driving it in-process.

- [ ] PR: cargo PM library split

### Step 2: Carry workspace information over the protocol

Add the workspace root, member directories, and crate list to the protocol, and move Symposium's readers onto it.

- [x] PR: workspace info over the wire

### Step 3: `symposium-pm-cargo` binary

Wrap the library in the SDK's server harness. Forward the cargo binary override so the test harness's fake cargo still applies.

- [ ] PR: cargo PM binary

### Step 4: Switch Symposium to the subprocess

Replace the in-process instance with the spawned one. Measure the hook path before and after; confirm `Cargo.lock`-unchanged still means no resolution.

- [ ] PR: cargo PM cutover + benchmark
