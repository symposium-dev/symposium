# Custom registries

## TL;DR

Allow plugins to define new registry types (e.g., `source.npm`, `source.pypi`) so that non-Rust ecosystems can be used as plugin sources. A registry is a long-lived process speaking JSON-RPC that handles resolution, predicate evaluation, discovery, and validation for its keys.

## Motivation

Only `path`/`paths` is truly built-in. The `git` and `cargo` registries are initially built-in for bootstrapping but designed to be replaceable by custom registry plugins. Symposium aims to support any language ecosystem — an npm plugin could teach Symposium how to resolve `source.npm = "my-plugin"`, a Python plugin could handle `source.pypi = "my-plugin"`, etc.

This also enables enterprise-specific registries (internal artifact stores, proprietary package managers) without requiring changes to Symposium's core.

## Change in a nutshell

### What a registry defines

A registry plugin declares:

- **Package kinds** — keys that appear under `source.*`, `where.*`, and `discovery.{allow,deny}.*`. For example, `symposium-cargo` defines the key `cargo`.
- **Predicates** (optional) — additional predicate names usable in `where.predicates`. These are pure predicates with no corresponding `source.*` resolver (e.g., a predicate that checks a project property without fetching a package).

Key names cannot overlap with builtins (`predicates`, `predicate`, `path`, `paths`) or keys from any other loaded registry.

```toml
# In a plugin's SYMPOSIUM.toml
[[registries]]
name = "cargo"
keys = ["cargo"]
command = "symposium-cargo"
```

### Where registry keys appear

A registry's keys appear uniformly in three contexts:

| Context | Example | Meaning |
|---------|---------|---------|
| `source.<key>` | `source.cargo = "my-plugin"` | Fetch this package from the registry |
| `where.<key>` | `where.cargo = { serde = "*" }` | Is this package a dep of the current project? |
| `discovery.allow.<key>` | `allow.cargo = { serde = "*" }` | These packages are eligible for auto-discovery |
| `discovery.deny.<key>` | `deny.cargo = { sketchy = "*" }` | These packages are ineligible |

Symposium does not interpret the values — they are opaque TOML, routed to the registry that owns the key.

### Singular and plural

Both singular values and arrays/tables are accepted where the registry supports them (e.g., `source.git = "url"` and `source.git = ["url1", "url2"]`). The registry decides how to interpret its values.

### Protocol

Symposium communicates with registry plugins via JSON-RPC (using the framework from the `agent-client-protocol` crate). The registry runs as a long-lived process, spawned once per sync.

**Operations:**

**`validate(key, value)`** — Check that a key/value pair is well-formed. Called during `cargo agents plugin validate`.

```
-> { method: "validate", params: { key: "cargo", value: "symposium-rtk" } }
<- { result: { valid: true } }
```

**`where(key, value, context)`** — Evaluate a predicate. Returns true/false. The `context` includes the workspace root (the registry knows how to find lock files, manifests, etc. from there).

```
-> { method: "where", params: { key: "cargo", value: { serde: ">=1.0" }, context: { workspace_root: "/path/to/project" } } }
<- { result: { matches: true } }
```

**`source(key, value, cache_dir)`** — Resolve a package to a local directory. The registry handles downloading, caching, and version resolution. Returns the path to the resolved plugin directory.

```
-> { method: "source", params: { key: "cargo", value: { serde: "1" }, cache_dir: "/home/me/.symposium/cache" } }
<- { result: { path: "/home/me/.symposium/cache/cargo/serde-1.0.219" } }
```

**`discover(key, policy, context)`** — Given accumulated allow/deny policy and workspace context, return the list of packages that should be auto-discovered as plugins. The policy values are opaque (collected from `discovery.allow.<key>` and `discovery.deny.<key>` across all loaded plugins and user config).

```
-> { method: "discover", params: { key: "cargo", allow: { serde: "*", tokio: "*" }, deny: { sketchy: "*" }, context: { workspace_root: "/path/to/project" } } }
<- { result: { packages: [{ key: "cargo", value: "serde" }, { key: "cargo", value: "tokio" }] } }
```

### Relationship to existing custom predicates

Existing custom predicates (defined via `[[custom_predicates]]` in SYMPOSIUM.toml) are a degenerate case: a registry that defines only predicates, with no `source` or `discover` capability. The current custom predicate mechanism can be viewed as a simplified form of this protocol.

### Flow

1. Load plugins from config + workspace.
2. Discover registries from loaded plugins. Spawn their processes.
3. Accumulate `discovery.allow`/`discovery.deny` policy, grouped by registry key.
4. For each registry with accumulated policy, send `discover` with the policy and workspace context.
5. Load discovered plugins (which may define more policy — loop until stable).
6. During graph expansion, `source.*` and `where.*` calls are routed to the appropriate registry process.

### `cargo agents info`

The existing `cargo agents crate-info` command generalizes to `cargo agents info` with registry-generic syntax:

```bash
cargo agents info serde           # shorthand for cargo=serde (cargo is the default)
cargo agents info cargo=serde     # explicit
cargo agents info npm=express     # future: routes to the npm registry
```

This is just the `source` operation exposed to the user — "resolve this package to a cached directory and show me what's in it." The registry handles fetching and caching; Symposium scans the result and displays the plugin contents (skills, hooks, MCP servers, sub-plugins).

### Cache validity

The `source` operation returns a path to a cached directory, but the protocol does not yet define how long that path remains valid. For now, cached results are considered valid until the next sync. Eventually we may want registries to declare TTLs or support explicit refresh, but this is deferred.

### Security

Custom registries execute arbitrary code during sync. This is consistent with the existing trust model — hooks already run arbitrary code, and users explicitly install registry plugins. The trust boundary is: installing the plugin (via `cargo agents use` or config) is the opt-in.

## Implementation plan

To be designed in detail once the built-in registry model stabilizes and we have concrete demand for a second ecosystem. The JSON-RPC framework from `agent-client-protocol` provides the transport layer.

Key milestones:
1. Define the JSON-RPC schema for all four operations.
2. Refactor builtin `cargo`/`git` registries to use the same internal interface (proving the abstraction).
3. Add `[[registries]]` parsing to SYMPOSIUM.toml.
4. Implement process lifecycle management (spawn per-sync, graceful shutdown).
5. Wire into the graph walker's source resolution and predicate evaluation paths.

## Tests

### Parsing

- `registries_entry_parses` — `[[registries]] name = "npm" keys = ["npm"] command = "symposium-npm"` parses.
- `registry_key_conflict_with_builtin_errors` — key `path` conflicts with builtin; parse error.
- `registry_key_conflict_between_plugins_errors` — two plugins claim key `npm`; error.

### Protocol (mock registry)

- `validate_request_format` — verify JSON-RPC request shape for `validate`.
- `where_request_format` — verify JSON-RPC request shape for `where`.
- `source_request_format` — verify JSON-RPC request shape for `source`.
- `discover_request_format` — verify JSON-RPC request shape for `discover`.
- `source_returns_path_and_plugin_loads` — mock registry returns path; plugin loaded from that path.
- `where_returns_true_activates_content` — mock registry says `matches: true`; gated content activates.
- `where_returns_false_skips_content` — mock says `matches: false`; content skipped.

### Discovery via custom registry

- `custom_registry_discover_returns_packages` — mock's `discover` returns packages; they enter the worklist.
- `discovery_policy_forwarded_to_registry` — accumulated `[discovery.allow]`/`[discovery.deny]` passed correctly.

### `cargo agents info`

- `info_bare_name_defaults_to_cargo` — `cargo agents info serde` routes to cargo registry.
- `info_explicit_registry_prefix` — `cargo agents info npm=express` routes to npm registry.
- `info_unknown_registry_errors` — `cargo agents info unknown=foo` produces helpful error.

### Lifecycle

- `registry_process_spawned_on_demand` — registry only started when its key is encountered.
- `registry_process_reused_across_calls` — multiple `source`/`where` calls go to same process.
- `registry_process_terminated_after_sync` — graceful shutdown after sync completes.
