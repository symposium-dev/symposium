# Registry-centric plugin distribution

## TL;DR

Generalize Symposium's plugin system around *package managers* (PMs). A plugin is identified by a canonical triple `pm:name:version`, fetched by its PM, and unpacked into a cached directory. Users install plugins with `cargo agents use`, projects auto-discover them via their dependencies, and predicates gate activation without changing what's installed.

## Motivation

**Leverage existing package managers.** Registries like crates.io already handle versioning, distribution, authentication, and mirroring. Enterprises already integrate them into their workflows. Rather than building our own distribution mechanism, we treat existing PMs as the delivery channel for plugins — keeping things simple for users and ops-free for us.

**Integrate across ecosystems.** Today Symposium only works with crates.io. We want to extend support to npm, PyPI, and beyond (including internal/proprietary registries). The PM abstraction makes each ecosystem a plug-in capability: implement four operations and your ecosystem's packages become plugin sources.

**Bundle executable code with plugin configuration.** Plugins can define hooks and MCP servers, but these need supporting binaries — a custom linter, a token-reduction tool like [RTK](https://github.com/rtk-ai/rtk/), a code generation tool. Today there's no clean way to distribute an executable alongside the TOML that references it. By connecting plugins to PMs, binaries and configuration ship together. The PM handles building and versioning; Symposium just fetches the directory and scans it.

## Core concepts

### Plugin identity: the `pm:name:version` triple

Every plugin has a canonical identifier — three colon-separated strings:

- **pm** — which package manager owns it (e.g., `cargo`, `npm`, `recommendations`, `git`)
- **name** — the package name within that PM's namespace
- **version** — arbitrary text whose meaning is defined by the PM (e.g., `1.2.3` for cargo, a commit SHA for git).

Examples:
- `cargo:serde-skills:1.2.3`
- `recommendations:cargo:serde:0.1.0`
- `git:github.com/rtk-ai/rtk:1.0.0-a3b2c1d`
- `npm:eslint-plugin-symposium:2.1.0`

### The package manager interface

A PM implements four operations:

| Operation | Input | Output | Used by |
|-----------|-------|--------|---------|
| `resolve` | opaque TOML value (from `source.<pm>`) | set of `pm:name:version` | manifest processing |
| `search` | partial query string (`pm?:name:version?`) | set of identifiers + metadata | `cargo agents use` |
| `fetch` | `pm:name:version` | directory with `Symposium.toml` + content | sync/install |
| `list-deps` | workspace directory | set of identifiers | auto-discovery |

- **`resolve`** takes the TOML value from a `source.<pm> = { ... }` entry and returns canonical triples. This is deterministic — given the same input and registry state, it returns the same result.
- **`search`** is the interactive/fuzzy version for CLI use. Partial queries match broadly.
- **`fetch`** downloads and unpacks the exact versioned content.
- **`list-deps`** inspects the workspace to report what the project depends on, expressed as identifiers relevant to this PM.

### Built-in PMs

#### `recommendations`

Ships with Symposium (the `symposium-recommendations` crate). A git repo containing directories keyed by plugin identifiers (which can reference other PMs' namespaces, e.g., `cargo:serde:*`). Its `list-deps` reads a project-level `Symposium.toml` to find explicit recommendations. Its `search` walks the repository. This is how Symposium provides curated, opinionated defaults.

#### `cargo`

Reads workspace `Cargo.toml`/`Cargo.lock` for `list-deps`. Searches crates.io for packages that contain a `Symposium.toml` or `skills/` directory. `fetch` downloads the crate source and extracts the plugin directory.

#### `git`

Fetches plugin directories from git repositories. The canonical version encodes the resolved commit SHA.

#### `path`

Local directories. Identity is filesystem-based. Primarily for development and workspace-local plugins.

### Plugins

A *plugin* is a directory containing:

- An optional `Symposium.toml` manifest
- Optional skills (`skills/`, `.agents/skills/`)
- Optional hooks, MCP servers, and other agentic extensions
- Optional sub-plugins (nested directories with their own manifests)

We favor *convention over configuration*. A directory with just a `skills/` subdirectory (and no `Symposium.toml`) is a valid plugin.

#### Sub-plugins

A plugin's manifest can declare dependencies on other plugins via `[[plugins]]` entries. Installing a plugin installs its sub-plugins transitively. Predicates on sub-plugins still apply (they're installed but may not activate).

#### Installed vs. active

A plugin is *installed* when its content is in the local cache. A plugin is *active* when its predicates evaluate to true in the current workspace. Sync only wires active plugins into agent directories — but all installed plugins are available for activation without re-fetching.

### Predicates

Predicates gate *activation*, not installation. They are flat — no per-PM namespace tables:

```toml
predicates = ["depends-on(serde)", "path-exists(build.rs)"]
```

Or the shorthand:

```toml
depends-on = ["serde", "cargo:tokio"]
```

The `depends-on` predicate can be bare (matched across all PMs' `list-deps`) or scoped to a specific PM (`depends-on(cargo:serde)`).

Predicates can appear at any level: plugin, skill group, individual skill, hook, MCP server. A predicate on a plugin means none of its contents activate (but sub-plugins with passing predicates still can).

Available predicates include:
- `depends-on(name)` / `depends-on(pm:name)` — workspace depends on this identifier
- `path-exists(path)` — file or directory exists
- `env(VAR)` / `env(VAR=value)` — environment variable check
- `shell(command)` — arbitrary shell check (exit 0 = true)
- `not(p)`, `any(p, ...)`, `all(p, ...)` — combinators

### State model

Three layers of state:

1. **Config** (`~/.symposium/config.toml`) — version *requirements* for root plugins. Written by `cargo agents use`. Example: `["cargo:serde-skills:1.*", "recommendations:*:*"]`. These are the user's explicit choices.

2. **Cache** (`~/.symposium/cache/`) — unpacked plugin directories on disk, keyed by canonical `pm:name:version`. Rebuilt from config + resolution at any time. This is the materialized result of `fetch`. Also stores sub-plugins.

3. **Agent directories** (`.claude/skills/`, `.agents/skills/`, etc.) — the synced output. Only active (predicates-pass) content from cached plugins is copied here.

Config stores roots with version ranges. Cache stores resolved, exact versions. Agent dirs store the activated subset.

## Use cases

After the user runs `cargo install symposium` and then `cargo agents init`:

### Dependency discovery

The user starts their agent in a project. The agent invokes a hook which advises them that there are new extensions available for some of their dependencies. The user runs `cargo agents sync`, picks the ones they want, and they are installed.

Flow:
1. Walk all PMs, call `list-deps` on each → set of identifiers the project depends on
2. For each identifier, `search` across PMs for matching plugins (including recommendations)
3. For new matches not yet installed: prompt the user to pick which to install
4. Fetch and cache chosen plugins
5. Evaluate predicates → determine active set
6. Sync active skills/hooks/MCP servers to agent directories

### Global use

User runs `cargo agents use --global C`. We search through PMs for packages named `C` (the user can also do `cargo agents use --global cargo:C` to be explicit). The plugin is installed and available everywhere.

### Local use

User runs `cargo agents use C`. We search through PMs for packages named `C` (or `cargo agents use cargo:C`). The plugin is installed and available in that workspace directory. Does not modify any files in the workspace — config lives in `~/.symposium/`.

### Workspace plugins

A project has a `Symposium.toml` at its workspace root or in a crate within the workspace. It specifies skills, hooks, `[[plugins]]`, etc. These are always active for developers working in that project. Consumers of the library get a curated subset (gated by predicates).

### Implicit plugins (convention over configuration)

A missing `Symposium.toml` is equivalent to an empty one. Symposium applies defaults:
- `skills/<name>/SKILL.md` files are discovered as skills
- `.agents/skills/<name>/SKILL.md` files are discovered as skills

No explicit configuration is needed to expose skills — just place them in the conventional directories.

## Key commands

- **`cargo agents use [--global] X`** — searches all PMs for `X`, presents options if ambiguous, records the version requirement in config (global or workspace-scoped).
- **`cargo agents remove X`** — removes from config.
- **`cargo agents sync`** — resolves config requirements, runs discovery, installs new plugins (prompting or auto-installing), evaluates predicates, syncs active content to agent dirs.
- **`cargo agents status`** — shows what's installed, what's active, and why.

## Enterprise control

The primary mechanism for enterprise customization is **overriding the crates that provide built-in PMs**:

- **`symposium-recommendations`** — provides the recommendations PM. A corporate fork curates the approved plugin set, vouches for internal tooling, removes community plugins that don't meet policy.
- **`symposium-cargo`** — provides the cargo PM. A corporate fork can point at an internal registry mirror, restrict which crates are searchable, etc.

This works because the PM interface is the seam — the binary loads whatever PM implementations are installed. No special enterprise configuration syntax needed; just ship your own crate.

**Policy plugin (orthogonal).** In addition to PM overrides, organizations can provide a policy plugin that enforces rules across the system — deny-listing specific plugins, requiring approval before installation, restricting what can activate in certain environments. This is a separate, overridable extension point (e.g., a `symposium-policy` crate). Design TBD.

## Summary of changes from today

| Change | Kind | Area |
|--------|------|------|
| `source = "crate"` on skill groups | Removed | Plugin model |
| `[[plugin-source]]` in config | Removed | Plugin model |
| `[defaults]` in config (booleans) | Removed | Plugin model |
| `self-contained = true` in config | Removed | Plugin model |
| `crates:` in SKILL.md frontmatter | Removed | Predicates |
| `PredicateOutput.selected_crates` in symposium-sdk | Removed | Plugin model |
| `crate_metadata.rs`, `load_crate_skills`, `fetch_and_resolve_skills`, `union_matched_crates` | Removed | Plugin model |
| `where.{cargo,predicates}` namespaced tables | Removed | Predicates |
| `[[plugin-source]]` → root `[[plugins]]` in config | Renamed | Plugin model |
| `crates = [...]` → `depends-on = [...]` / `predicates = ["depends-on(...)"]` | Renamed | Predicates |
| `cargo agents crate-info` → `cargo agents info` | Renamed | Custom PMs |
| Package manager abstraction (4 operations) | Added | Plugin model |
| `pm:name:version` canonical identity | Added | Plugin model |
| `cargo agents use` / `cargo agents remove` | Added | User-managed plugins |
| `cargo agents status` | Added | User-managed plugins |
| Config as version requirements (not exact pins) | Added | Plugin model |
| Cache layer (unpacked plugins keyed by triple) | Added | Plugin model |
| Auto-discovery via PM `list-deps` + `search` | Added | Discovery |
| Sub-plugins (transitive installation) | Added | Plugin model |
| Installed vs. active distinction | Added | Plugin model |
| Convention-based plugin defaults | Added | Plugin defaults |
| Custom PMs defined by plugins | Added | Custom PMs |

## Detailed design

### Sub-RFDs

#### [PM interface](./pm-interface/README.md)

The core abstraction. Protocol for PM implementations (trait? CLI binary? JSON-RPC?), error semantics, caching contract, how `resolve`/`search`/`fetch`/`list-deps` work in detail. Includes the built-in PMs (cargo, recommendations, git, path) as concrete implementations showing the interface in action.

#### [Discovery & sync](./discovery-sync/README.md)

The hook-triggered discovery flow, prompt UX, how recommendations vouch for deps by keying on other PMs' identifiers (`cargo:serde:*`), auto-install configuration, behavior when multiple PMs return matches, ordering of operations.

#### [User-managed plugins](./user-managed-plugins/README.md)

`cargo agents use`/`remove`/`status` commands. Config file format, version requirement syntax, global vs. workspace-local scoping (local installs don't modify workspace files).

### Predicates (inline)

Predicates are flat expressions gating activation. Grammar:

```
predicate   = function "(" args ")"
function    = "depends-on" | "path-exists" | "env" | "shell" | "not" | "any" | "all"
args        = (predicate | string) ("," (predicate | string))*
```

Available predicates:
- `depends-on(name)` / `depends-on(pm:name)` — workspace depends on this identifier (checked via PMs' `list-deps`)
- `path-exists(path)` — file or directory exists relative to workspace root
- `env(VAR)` / `env(VAR=value)` — environment variable check
- `shell(command)` — exit 0 = true
- `not(p)`, `any(p, ...)`, `all(p, ...)` — combinators

Shorthand in TOML: `depends-on = ["serde", "cargo:tokio"]` is sugar for `predicates = ["depends-on(serde)", "depends-on(cargo:tokio)"]`.

Predicates can appear at any level (plugin, skill group, skill, hook, MCP server). A predicate on a plugin gates all its contents but not its sub-plugins (which have their own predicates).

### Plugin defaults (inline)

Convention-over-configuration rules applied to every plugin directory:

- A missing `Symposium.toml` is equivalent to an empty one.
- `skills/<name>/SKILL.md` files are discovered as skills.
- `.agents/skills/<name>/SKILL.md` files are discovered as skills.
- A co-located `Cargo.toml` with `[[bin]]` targets provides implicit installations (hooks/MCP servers can reference them by name without explicit `[installation]` entries).

### Future work

- **Fixed-point resolution** — a convergence loop for when plugins define custom predicates that other plugins depend on. Needed once custom predicates are used across plugin boundaries.
- **Custom PMs** — allowing plugins to define new PM types (npm, pypi, internal). The PM interface is the seam; registration and discovery mechanism TBD.
- **Policy plugins** — org-level enforcement (deny-lists, approval gates). Separate extension point, design TBD.

## Implementation order

1. **PM interface** — the foundation. Identity triple, four operations, state layers, sub-plugins.
2. **Predicates** — lands alongside. Flat syntax, `depends-on`.
3. **Plugin defaults** — convention-over-configuration. "Just drop skills in a directory."
4. **Discovery & sync** — PM `list-deps` + `search`, prompt UX. Enables "plugins from your dependencies."
5. **User-managed plugins** — `use`/`remove`/`status` UX.
6. **Future work** — fixed-point, custom PMs, policy — as demand arises.
