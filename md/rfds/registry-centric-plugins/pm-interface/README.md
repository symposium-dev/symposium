# PM interface

## TL;DR

- Define a four-operation interface (`resolve`, `search`, `fetch`, `list-deps`) that all package managers implement.
- PMs are separate binaries communicating via JSON-RPC over stdio.
- Only `path` is built into the Symposium binary; `cargo`, `git`, and future PMs are external.

## Motivation

Symposium needs to fetch plugins from multiple ecosystems without hard-coding each one. The PM interface is the seam: implement four operations and your ecosystem becomes a plugin source. This lets us ship cargo support today, add npm/pypi later, and let enterprises plug in internal registries — all without changing core.

## Change in a nutshell

A PM is a separate binary that speaks JSON-RPC over stdio. Here's the cargo PM responding to `resolve`:

```toml
# User writes in Symposium.toml:
[[plugins]]
source.cargo = { serde-skills = "1" }
```

Symposium passes `{ "serde-skills": "1" }` to the cargo PM's `resolve` method. It queries crates.io and returns `(cargo, serde-skills, 1.2.3)`.

Then `fetch((cargo, serde-skills, 1.2.3))` downloads the crate and unpacks the plugin directory into cache.

## Detailed plans

### Package-ids

A **package-id** is a tuple `(pm, name, version)` where all three components are PM-defined strings. There is no mandated string-serialized format — the tuple is the identity.

Examples:
- `(cargo, serde, 1.0.210)`
- `(git, git@github.com:rtk-ai/rtk#main, abc123def)`
- `(recommendations, cargo/serde, 0.1.0)`

In the JSON-RPC protocol, a package-id is represented as:

```json
{ "pm": "cargo", "name": "serde", "version": "1.0.210" }
```

### The protocol

PMs are separate binaries invoked by Symposium. Communication uses JSON-RPC over stdio (the same pattern as MCP servers). Each PM binary is long-lived — Symposium spawns it once and sends multiple requests.

The protocol defines four methods:

#### `resolve`

```json
// Request
{ "method": "resolve", "params": { "value": { "serde-skills": "1" } } }

// Response
{ "result": [{ "pm": "cargo", "name": "serde-skills", "version": "1.2.3" }] }
```

Takes the opaque TOML value from `source.<pm> = { ... }` (passed as JSON). Returns a set of package-ids.

- Cargo PM: `{ "serde-skills": "1" }` → queries registry → `(cargo, serde-skills, 1.2.3)`
- Git PM: `{ "url": "...", "branch": "main" }` → resolves ref → `(git, git@github.com:org/repo#main, abc123)`
- Path PM (built-in, not JSON-RPC): `{ "path": "./my-plugin" }` → canonicalizes

May involve network calls. Deterministic given same registry state.

#### `search`

```json
// Request
{ "method": "search", "params": { "query": { "pm": "cargo", "name": "serde", "version": "1.0.210" } } }

// Response
{ "result": [{ "id": { "pm": "cargo", "name": "serde-skills", "version": "1.2.3" }, "description": "..." }] }
```

Takes a package-id tuple (all fields provided — as returned by another PM's `list-deps`). Returns matching plugins from this PM's perspective.

- Each PM decides which tuple components to match on. The recommendations PM ignores version; the cargo PM matches on name.
- If the query's `pm` field doesn't relate to this PM, it may return empty.
- Used during discovery: `list-deps` results are passed as queries to every PM's `search`.

#### `fetch`

```json
// Request
{ "method": "fetch", "params": { "id": { "pm": "cargo", "name": "serde-skills", "version": "1.2.3" }, "dest": "/home/user/.symposium/cache/cargo/serde-skills/1.2.3" } }

// Response
{ "result": { "path": "/home/user/.symposium/cache/cargo/serde-skills/1.2.3" } }
```

Downloads exact versioned content into the provided destination directory.

Contract:
- Same package-id always produces same content.
- PM writes into `dest`, which Symposium provides.
- If `dest` already has content, PM may skip (cache hit).

#### `list-deps`

```json
// Request
{ "method": "list_deps", "params": { "workspace": "/home/user/projects/my-app" } }

// Response
{ "result": [{ "pm": "cargo", "name": "serde", "version": "1.0.210" }, { "pm": "cargo", "name": "tokio", "version": "1.38.0" }] }
```

Inspects the workspace and reports dependencies relevant to this PM. Returns full package-id tuples.

Contract:
- Direct dependencies only (not transitive).
- Must be fast — called on every sync. Read lockfiles, don't query the network.

### Error handling

Errors use JSON-RPC error codes:

| Code | Meaning | Symposium behavior |
|------|---------|-------------------|
| -32001 | Not found | Skip gracefully, report in `status` |
| -32002 | Network error | Retry with backoff, fall back to cache |
| -32003 | Invalid input | Hard error at parse time |
| -32004 | Auth required | Report to user with setup instructions |

### PM lifecycle

Symposium manages PM binaries as follows:

1. PMs are installed as `[[installable]]` entries — from the recommendations repository or the user's root config.
2. On first use, Symposium spawns the PM binary and connects via stdio.
3. The PM stays alive for the duration of the sync/hook operation.
4. Symposium may call methods concurrently (the PM should handle this or serialize internally).

The `path` PM is the exception — it's built into the Symposium binary itself (since it just reads local directories and has no external dependencies).

### Cache layout

```
~/.symposium/cache/
├── cargo/
│   └── serde-skills/
│       └── 1.2.3/
│           ├── Symposium.toml
│           └── skills/
├── git/
│   └── github.com-org-repo/
│       └── abc123/
│           └── ...
└── recommendations/
    └── cargo/
        └── serde/
            └── ...
```

Cache is a pure optimization — deletable and rebuildable from config. Symposium owns the directory structure; PMs write content into the slot they're given.

### Built-in PMs

#### `path`

Built into the Symposium binary. For local development and workspace-local plugins.

- `resolve`: canonicalizes a path, returns `(path, /absolute/path, _)`.
- `search`: returns empty (not a searchable registry).
- `fetch`: no-op (content is already on disk).
- `list-deps`: returns empty.

### External PMs (shipped as installables)

#### `cargo`

Separate binary (`symposium-pm-cargo`). See the [cargo PM sub-RFD](../cargo-pm/README.md) for details.

#### `git`

Separate binary (`symposium-pm-git`). Resolves refs to commit SHAs, fetches repo content.

#### `recommendations`

Separate binary (`symposium-pm-recommendations`). Operates over the curated recommendations repository. See the main README's [recommendations manager section](../README.md#example-the-recommendations-manager) for structure.

## Frequently asked questions

### Why JSON-RPC over stdio?

It's the same pattern used by MCP servers and LSP — well-understood, language-agnostic, and debuggable. We can use the `agent-client-protocol` SDK for the implementation. It also means PMs can be written in any language.

### Why not compile PMs into the binary?

Language-agnosticism. We want npm/pypi PMs eventually, and those may be best written in JS/Python. Even for Rust-based PMs, the binary boundary keeps the core small and lets PMs be updated independently.

### Why is `path` the only built-in?

It has no external dependencies and no protocol overhead would be justified for "return this local directory." Every other PM needs network access, registry-specific logic, or ecosystem tooling — better as separate binaries.

### Who resolves version requirements — Symposium or the PM?

The PM. When config says the user wants `serde-skills` version `1.*`, Symposium calls `resolve` with that constraint. The PM knows how to interpret version ranges for its ecosystem.

## Implementation plan and status

### Step 1: Define the JSON-RPC protocol schema

Document the four methods, their request/response shapes, and error codes. Publish as a schema that PM authors can validate against.

- [ ] PR: protocol schema definition

### Step 2: Implement the `path` PM (built-in)

Simplest case — validates the fetch/cache flow end-to-end without spawning an external process.

- [ ] PR: path PM implementation + tests

### Step 3: PM process management

Spawning, stdio connection, JSON-RPC framing, lifecycle management (start on demand, keep alive during sync).

- [ ] PR: PM process manager

### Step 4: Implement `symposium-pm-cargo`

First external PM. Port existing crate-fetch logic. Validates the full JSON-RPC round-trip.

- [ ] PR: cargo PM binary + tests

### Step 5: Implement `symposium-pm-git`

Resolves refs, fetches repos. Validates a second external PM works with the protocol.

- [ ] PR: git PM binary + tests

### Step 6: Implement `symposium-pm-recommendations`

Operates over the recommendations repository structure.

- [ ] PR: recommendations PM binary + tests
