# Registry-centric plugin distribution

## TL;DR

Generalize Symposium's plugin system around *registries* — a plugin is a directory fetched from a path, git repo, crate, or (eventually) other package ecosystems. This enables packaging executable code with plugin configuration, enterprise distribution via registry mirrors, and a path toward multi-language support.

## Motivation

**Leverage existing registries.** Registries like crates.io already handle versioning, distribution, authentication, and mirroring. Enterprises already integrate them into their workflows. Rather than building our own distribution mechanism, we should treat existing registries as the delivery channel for plugins — keeping things simple for users and ops-free for us.

**Integrate across ecosystems.** Today Symposium only works with crates.io. We want to extend support to npm, PyPI, and beyond (including internal/proprietary registries). By normalizing the config around generic "sources" (`source.{path,git,cargo,npm,...}`) we can add new registries without breaking changes.

**Bundle executable code with plugin configuration.** Plugins can define hooks and MCP servers, but these need supporting binaries — a custom linter, a token-reduction tool like [RTK](https://github.com/rtk-ai/rtk/), a code generation tool. Today there's no clean way to distribute an executable alongside the TOML that references it. By connecting plugins to package registries, binaries and configuration ship together. The registry handles building and versioning; Symposium just fetches the directory and scans it. This makes it practical to distribute tools like RTK that want to be installable across many agents.

## Change in a nutshell

### Model we are working towards

The plugin model we are working towards is as follows:

#### Plugins = packages in your languages' package manager(s).

You select and install plugins via the same registry that you pick the dependencies for your project (e.g., crates.io, pypi, etc).

#### Plugins come from three sources

1. **Symposium recommendations and plugins you explicitly selectly.** Symposium ships with default recommendations (the `symposium-recommendations` crate) and you can run `cargo agents use C` to install more.
2. **Your project's dependencies.** We scan your project's direct dependencies to find things that ought to be installed. Those extensions can either be bundled in the crates themselves or distributed externally via centralized crates like `symposium-recommendations`.
3. **Your project itself.** We scan your workspace to identify skills, hooks, etc that you should be using. *Consumers* of your library get a subset of those.

#### Everything is built on "plugins"

A *plugin* is a directory with an optional `SYMPOSIUM.toml` file and optional associated package configuration (e.g., `Cargo.toml`, `pyproject.toml`). Plugins can contain:

* custom registries (used to support cargo, pypi, etc);
* custom predicates (to test for when skills/mcp-servers/etc should be used);
* allow-lists;
* other plugins;
* and agentic extensions like skills, hooks, etc.

We favor *convention over configuration* for defining plugins, adopting standards that exist. So you can e.g. just put skills into `skills/...` or `.agents/skills`.

When defining hooks or MCP servers, you can reference the associated packages (e.g., binaries defined in `Cargo.toml`), or reference external packages. Symposium will build and install those packages and keep them up-to-date.

### User-facing documentation

Proposed user-facing documentation showing the target experience:

- [About page](./preview/about.md)
- [Auto-discovery](./preview/auto-discovery.md)
- [Workspace plugins](./preview/workspace.md)

### Key changes from today

* **Namespaced syntax.** All registry-specific nouns are scoped under tables:
  * `source.{path,git,cargo,...}` — identifies which registry to fetch from and the package name. `path` is builtin; the others (`git`, `cargo`, and eventually `npm`, `pypi`, etc.) are defined by registry plugins.
  * `where.{predicates,cargo,...}` — gates activation. `predicates` is builtin (a list of predicate expressions); `cargo` is provided by the cargo registry plugin. Future registries will add their own entries here (e.g., `where.npm`).
* **No more "plugin sources" as a distinct concept.** Today, `[[plugin-source]]` entries point at directories that *contain* plugins. In the new model, there are only plugins — and plugins can contain other plugins. `[[plugins]]` entries (in config or in a manifest) point directly at a plugin to load, not at a container.
* **Provenance predicates and defaults replace special-case behavior.** Every plugin carries a set of provenances (`used`, `workspace`, `dependency`) that explain *why* it was loaded. Things that were previously special-cased — like `.agents/skills/` only loading for workspace projects — are now expressed as regular `where` clauses (e.g., `where.predicate = "workspace()"`).
* **Discovery replaces `source = "crate"`.** The old `source = "crate"` mechanism required concepts like "explicitly matched crates" and "witness sets" — complexity within individual plugins. We replace it with *discovery*, a top-level concept for how workspace dependencies become plugins. Discovery is just another source of plugins (alongside config and the workspace itself), not something internal to a plugin's manifest.

## Summary of changes

| Change | Kind | Details |
|--------|------|---------|
| `source = "crate"` on skill groups | Removed | [Plugin model](./plugin-model/README.md) |
| `[[plugin-source]]` in config | Removed | [Plugin model](./plugin-model/README.md) |
| `[defaults]` in config (`symposium-recommendations`/`user-plugins` booleans) | Removed | [Plugin model](./plugin-model/README.md) |
| `self-contained = true` in config | Removed | [Plugin model](./plugin-model/README.md) |
| `crates:` in SKILL.md frontmatter | Removed | [Where clauses](./where-clauses/README.md) |
| `PredicateOutput.selected_crates` in symposium-sdk | Removed | [Plugin model](./plugin-model/README.md) |
| `crate_metadata.rs`, `load_crate_skills`, `fetch_and_resolve_skills`, `union_matched_crates` | Removed | [Plugin model](./plugin-model/README.md) |
| `crate`/`crates` → `cargo` (in `source.*`, `where.*`, `discovery.*`) | Renamed | [Where clauses](./where-clauses/README.md) |
| `[[plugin-source]]` → `[[plugins]]` | Renamed | [Plugin model](./plugin-model/README.md) |
| `crates = [...]` (bare) → `where.cargo = { ... }` | Renamed | [Where clauses](./where-clauses/README.md) |
| `predicates = [...]` (bare) → `where.predicates = [...]` | Renamed | [Where clauses](./where-clauses/README.md) |
| `cargo agents crate-info` → `cargo agents info` | Renamed | [Custom registries](./custom-registries/README.md) |
| `[[plugins]]` in manifests (transitive plugin loading) | Added | [Plugin model](./plugin-model/README.md) |
| `[[skills]]` entries with `where` + `source` | Added | [Plugin defaults](./plugin-defaults/README.md) |
| Provenance predicates: `workspace()`, `used()`, `dependency()` | Added | [Plugin model](./plugin-model/README.md) |
| `where.*` table on all gated constructs | Added | [Where clauses](./where-clauses/README.md) |
| `[discovery.allow]` / `[discovery.deny]` | Added | [Discovery policy](./discovery/README.md) |
| `[defaults]` in `SYMPOSIUM.toml` (suppress default entries) | Added | [Plugin defaults](./plugin-defaults/README.md) |
| `[[registries]]` in manifests (custom registries) | Added | [Custom registries](./custom-registries/README.md) |
| Default installations from `Cargo.toml` `[[bin]]` targets | Added | [Plugin defaults](./plugin-defaults/README.md) |
| Graph-based plugin loader (replaces flat `load_registry()`) | Added | [Plugin model](./plugin-model/README.md) |
| `workspace-directory()` predicate | Added | [User-managed plugins](./user-managed-plugins/README.md) |
| `cargo agents use` / `cargo agents remove` | Added | [User-managed plugins](./user-managed-plugins/README.md) |
| `cargo agents status` | Added | [User-managed plugins](./user-managed-plugins/README.md) |
| Synthesized empty `SYMPOSIUM.toml` when none present | Added | [Plugin defaults](./plugin-defaults/README.md) |
| Recursive directory search for skills and sub-plugins | Added | [Plugin defaults](./plugin-defaults/README.md) |

## Detailed design

This work is broken into independently designable (and largely independently implementable) components:

### [Plugin model](./plugin-model/README.md)

The core redesign. A plugin is a directory; registries fetch directories; plugins can transitively load other plugins via `[[plugins]]` entries. The flat plugin registry becomes a graph walker with dedup and provenance tracking. Also covers the config migration (`[[plugin-source]]` → `[[plugins]]`) and the removal of `source = "crate"`.

**Dependencies:** None (this is the foundation).

### [Where clause syntax](./where-clauses/README.md)

Move `crates` and `predicates` from bare top-level fields to `where.{cargo,predicates}`. Rename `crates`/`crate` to `cargo` (key names the ecosystem). The `where` table provides a namespace so registry-specific nouns won't collide with future top-level keys.

**Dependencies:** Plugin model (the new `[[plugins]]` entries use `where` from the start).

### [Discovery policy](./discovery/README.md)

Allow/deny rules controlling which workspace dependencies are automatically loaded as plugins. Default deny-all. Policy can come from user config or from already-loaded plugins (e.g., `symposium-recommendations` vouching for crates it knows).

**Dependencies:** Plugin model (discovery feeds candidates into the graph walker).

### [Plugin defaults](./plugin-defaults/README.md)

Synthesized manifests (empty `SYMPOSIUM.toml` when none exists), default `[[plugins]]`/`[[skills]]` entries, recursive directory search, and implicit binary installations from a co-located `Cargo.toml`.

**Dependencies:** Plugin model (defaults are injected during graph expansion).

### [User-managed plugins](./user-managed-plugins/README.md)

`cargo agents use`/`remove` commands with local-by-default scoping via a `workspace-directory()` predicate. Also `cargo agents status` for inspecting what's active and why.

**Dependencies:** Plugin model, where clauses.

### [Fixed-point resolution](./fixed-point-resolution/README.md)

A convergence loop for the plugin loader that handles custom predicates creating ordering dependencies between plugins. Needed once custom predicates are used across plugin boundaries.

**Dependencies:** Plugin model.

### [Custom registries](./custom-registries/README.md)

Allow plugins to define new registry types (e.g., `source.npm`, `source.pypi`) so non-Rust ecosystems can be used as plugin sources. An early sketch — to be fully designed once built-in registries stabilize.

**Dependencies:** Plugin model, fixed-point resolution (a custom registry defined by one plugin may be needed to load another).

## Dependency graph

```
plugin-model
├── where-clauses
│   ├── plugin-defaults (uses where.predicate = "workspace()")
│   ├── discovery (policy keys are registry-namespaced)
│   └── user-managed-plugins
├── fixed-point-resolution
│   └── custom-registries
└── discovery
```

## Implementation order

A suggested sequencing that keeps the codebase working at each step:

1. **Plugin model** — the foundation; everything else builds on it. Includes provenance predicates (`workspace()`, `used()`, `dependency()`).
2. **Where clauses** — lands alongside or immediately after, since new `[[plugins]]` entries use `where` from day one.
3. **Plugin defaults** — requires where-clauses (uses `where.predicate = "workspace()"`) and provenance predicates. Makes the workspace-as-plugin story work.
4. **Discovery** — requires where-clauses (policy keys use the same registry namespace). Enables the "plugins from your dependencies" story.
5. **User-managed plugins** — requires where-clauses. The `use`/`remove` UX.
6. **Fixed-point resolution** — only needed once real-world plugins define cross-boundary custom predicates.
7. **Custom registries** — future; to be designed once built-in registries stabilize and we have demand for a second ecosystem.
