# Plugin model

## TL;DR

- A plugin is a directory with an (optional) `SYMPOSIUM.toml`.
- Plugins are fetched via registries (`source.{path,git,cargo}`).
- Plugins can transitively load other plugins via `[[plugins]]` entries.
- The plugin loader becomes a graph walker with dedup and provenance tracking.

## Change in a nutshell

### Plugin = directory

A Symposium plugin is identified by a directory `P` with an (optional) `SYMPOSIUM.toml` file at its root (`P/SYMPOSIUM.toml`). If the `SYMPOSIUM.toml` file is not present, we synthesize an empty one (all fields are optional). If the `SYMPOSIUM.toml` file is not present, it is treated as if there is an empty file. After the `SYMPOSIUM.toml` file is parsed, default entries are added unless explicitly suppressed ([described in a separate RFD](../plugin-defaults/README.md)).

### Plugin registries

Plugins can be found via *registries*. A registry maps from some kind of "plugin name" to a plugin directory and arranges for it to be made available for the symposium tool. In the configuration, the `source` object is used to identify the registry and the plugin name (e.g., `source.$registry = $name`).

Only `path`/`paths` is truly built-in (direct filesystem access, no external tool needed). The `git` and `cargo` registries are initially implemented as built-in for bootstrapping convenience, but are designed to be replaceable by [custom registry plugins](../custom-registries/README.md) once that infrastructure lands. Future registries (`npm`, `pypi`, etc.) will be custom from the start.

| Registry type | How it resolves | Example |
|---|---|---|
| `source.path`, `source.paths` | Direct filesystem path(s) | `source.path = "/home/me/my-plugin"` |
| `source.git` | Clone/fetch a git repository | `source.git = "https://github.com/org/plugins"` |
| `source.cargo` | Fetch from a cargo registry | `source.cargo = "my-plugin"` |

**Singular/plural conventions:**
- `source.path` takes a string; `source.paths` takes an array of strings.
- `source.git` accepts either a string or an array of strings (the registry handles both).
- `source.cargo` accepts a string, a table (`{ serde = "1" }`), or a table with multiple entries.

**Note: `crate`/`crates` → `cargo`.** We rename from `crate`/`crates` to `cargo` to align with the registry name (matching the pattern of `npm`, `pypi`, etc. — the key names the ecosystem, not the unit). This is a migration from the existing codebase.

### Loading plugins via `[[plugins]]`

`[[plugins]]` sections are used to load plugins from registries. They can appear in two places:

1. In `~/.symposium/config.toml` — the user's global config.
2. Inside a plugin's `SYMPOSIUM.toml` — allowing plugins to transitively pull in other plugins.

**Example.** When the user runs `cargo agents init`, we create a default user configuration with the following:

```toml
# In ~/.symposium/config.toml

[[plugins]]
source.cargo = { symposium-recommendations = "1" }
```

This ensures that the `symposium-recommendations` plugin is loaded globally from crates.io (we take the most recent `1.x` version at all times).

**Note: distribution mechanism change.** Today `symposium-recommendations` is fetched from a hardcoded git URL. We move it to a crate on crates.io. This is intentional — registry-based distribution means enterprises with private crate mirrors or alternate registries can substitute their own recommendations crate, controlling what plugins their developers receive without forking anything.

The source for that crate itself contains a `SYMPOSIUM.toml` file at its root which may load additional plugins like `symposium-rtk` (see example below). `symposium-rtk` would then be added to the global plugin set whenever `symposium-recommendations` is loaded.

```toml
# In the source for symposium-recommendations (crates.io)

[[plugins]]
source.cargo = "symposium-rtk"
```

### Plugin provenance

When Symposium starts it identifies a set of active plugins. These are derived from three root locations, called the *provenance* of a plugin:

1. **Explicitly "used" plugins** — this includes the `symposium-recommendations` crate and anything added via [`cargo agents use`](../user-managed-plugins/README.md). Tracked in `[[plugins]]` entries in user config.
2. **The active workspace** — the workspace root and member crates are always scanned as plugin sources.
3. **Workspace dependencies** — crates in your dependency graph may contain embedded plugins. An allow/deny list controls which are pulled in (to avoid loading arbitrary code from transitive deps).

A given plugin may be reached from multiple provenances. If one plugin P includes a `[[plugins]]` section that causes `Q` to be loaded, then `Q` inherits the provenance(s) of `P`.

**Example: transitive provenance.** The `symposium-recommendations` crate has provenance `used`. Since it includes `symposium-rtk`, that crate too would have provenance `used`. If a `SYMPOSIUM.toml` is found in the current workspace root, that plugin would have provenance `workspace`.

**Example: multiple provenances.** Suppose `dial9` is listed in the user's `[[plugins]]` config (provenance `used`) and the current workspace also has a dependency on `dial9` that passes the discovery policy. Then `dial9` has both `used` and `dependency` provenance simultaneously.

### Provenance predicates

Plugins may include predicates that test whether the plugin was reached through a given provenance:

* `workspace()` — true if the plugin was found in the active workspace.
* `used()` — true if the plugin was explicitly "used" via `cargo agents use`.
* `dependency()` — true if the plugin was found via a workspace dependency.

More than one of these predicates may be true within a given plugin.

## Detailed design

### Current architecture

Today, plugin loading is a flat, single-pass process:

1. `config.plugin_sources()` builds a list of source directories (from `[[plugin-source]]` entries + built-in defaults).
2. `load_registry()` iterates those directories, scanning each one for `SYMPOSIUM.toml` files. Each produces a `ParsedPlugin`.
3. Skill resolution (`skills.rs`) processes each plugin's skill groups. For `source = "crate"` groups, it uses `union_matched_crates()` to determine which workspace deps to fetch, then fetches and scans them recursively via `crate_metadata.rs`.
4. The result is a flat `PluginRegistry` — there is no mechanism for one plugin's manifest to pull in another plugin.

### Target architecture

The new model replaces flat loading with a graph-based resolution:

1. **Seeding.** The worklist is seeded from `[[plugins]]` entries in user config (evaluating their `where` clauses) and from the workspace root/members (unconditional).
2. **Expansion.** For each item in the worklist: resolve the source (fetch from registry if needed), scan the directory for `SYMPOSIUM.toml`, apply defaults, then examine the plugin's own `[[plugins]]` entries. Entries whose `where` clauses pass are added to the worklist; others are skipped.
3. **Dedup.** A plugin may be reachable from multiple paths. Track identity by resolved directory (after canonicalization). When a plugin is reached again, merge provenances and re-evaluate its `[[plugins]]` entries — the new provenance may cause previously-false `where` clauses to pass, triggering new items.
4. **Provenance propagation.** Each worklist item carries the provenance(s) of its parent. The workspace root carries `workspace`, config entries carry `used`, discovery candidates carry `dependency`.
5. **Discovery.** After all explicit sources are resolved, workspace dependencies are checked against the accumulated discovery policy (allow/deny rules collected from loaded plugins + user config). Approved deps enter the worklist with `dependency` provenance.

The key code changes:
- `load_registry()` becomes a graph walker, not a flat iterator.
- `PluginRegistry` gains a provenance set per plugin.
- The `source = "crate"` path (`load_crate_skills`, `fetch_and_resolve_skills`, `crate_metadata.rs`) is deleted entirely — crate-sourced plugins are just items in the worklist resolved via the crate registry.
- Skill resolution becomes simpler: no special `PluginSource::Crate` arm, just `discover_skills()` on the already-resolved directory.

### Changes to `cargo agents plugin` commands

The current `cargo agents plugin` subcommands are organized around "providers" (source directories). With the new model, plugins form a graph — there's no flat list of providers. The commands change accordingly:

**`cargo agents plugin list`** — Currently shows providers and their plugins. In the new model, shows the resolved plugin graph: each plugin, its source (crate, git, path, or workspace), its provenance, and whether it's active (predicates passing) or inactive. Example:

```
symposium-recommendations  cargo:symposium-recommendations@1.4.2  used
  symposium-rtk            cargo:symposium-rtk@0.3.1              used (transitive)
  symposium-serde          cargo:symposium-serde@1.0.0            used (transitive)
workspace root             path:/home/me/dev/my-project           workspace
my-internal-api            dependency (via discovery)             dependency
```

**`cargo agents plugin show <name>`** — Shows the resolved manifest for a single plugin (after defaults are applied), its provenance, active predicates, contributed skills/hooks/MCP servers.

**`cargo agents plugin validate <path>`** — Validates a `SYMPOSIUM.toml` (or directory) against the new schema. Reports errors for removed syntax (`source = "crate"`, bare `crates`, bare `predicates`) with migration suggestions.

## Removed syntax

The following existing constructs are removed in favor of the new model:

| Removed | Replacement | Rationale |
|---------|-------------|-----------|
| `source = "crate"` on skill groups | Discovery policy (`[discovery.allow]`) | Previously this was how a plugin "opted in" to scanning workspace deps. Now discovery is a separate policy concern, not a source type. |
| `[[plugin-source]]` in config | `[[plugins]]` with `source.*` | Same normalization; the old name implied "where to look" rather than "what to load." |
| `crates:` in SKILL.md frontmatter | Removed entirely | Per-skill crate gating is better expressed at the skill-group level via `where.cargo`. |
| `self-contained = true` in config | Removed (dead code) | Never wired up; the intent is covered by simply not including a `symposium-recommendations` entry in `[[plugins]]`. |

The `source = "crate"` removal is the most significant. Previously, a skill group declared `source = "crate"` to mean "resolve my predicates against workspace deps, fetch matching crates, and scan them for skills." This conflated three concerns: activation gating (predicates), source resolution (fetch from registry), and discovery authorization (which deps are safe to scan). The new model separates them: `where.cargo` gates activation, `source.cargo` fetches from a registry, and `[discovery.allow]` controls which workspace deps are eligible.

The `symposium-sdk` crate currently exports `PredicateOutput` with `selected_crates: Vec<SelectedCrate>` for custom predicate authors to emit witness data driving `source = "crate"` resolution. This field is removed (breaking change to the SDK). The SDK is not widely used and has no stability guarantee yet.

## Implementation plan

### Step 1: Config schema migration

Replace `[[plugin-source]]` + `[defaults]` with `[[plugins]]` array-of-tables using `source.*` and `where.*`. Old config format is a hard error — users must re-run `cargo agents init`. Add `#[serde(deny_unknown_fields)]` to config structs to prevent dead fields from accumulating unnoticed.

### Step 2: Graph-based plugin loading

Replace `load_registry()` with the worklist-based graph walker. Support `[[plugins]]` in manifests for transitive loading. Implement dedup and provenance tracking.

### Step 3: Provenance predicates

Add `workspace()`, `used()`, `dependency()` predicates. Wire provenance sets from the graph walker into predicate evaluation.

### Step 4: Remove `source = "crate"`

Delete the `source = "crate"` code path (`crate_metadata.rs`, `load_crate_skills`, `fetch_and_resolve_skills`, `union_matched_crates`, SDK witness fields).

## Tests

### Config parsing

- `parse_plugins_array_with_source_path` — `[[plugins]]` with `source.path` deserializes correctly.
- `parse_plugins_array_with_source_cargo` — `source.cargo = "foo"` and `source.cargo = { foo = "1" }` both parse.
- `parse_plugins_array_with_source_git` — `source.git = "https://..."` parses (string and array forms).
- `parse_plugins_array_with_where_clause` — `[[plugins]]` with `where.predicates` parses.
- `legacy_plugin_source_silently_migrates` — old `[[plugin-source]]` entries read as `[[plugins]]` with equivalent semantics.
- `deny_unknown_fields_on_config` — extra keys in config produce a parse error.
- `config_roundtrip_writes_new_format` — load old config, save, file contains `[[plugins]]` syntax.

### Graph walker

- `graph_walker_single_plugin` — one plugin, no children, resolves cleanly.
- `graph_walker_transitive_loading` — plugin A's manifest has `[[plugins]]` pointing at B; both load.
- `graph_walker_dedup_by_canonical_path` — same plugin reached from two paths loads once, provenances merged.
- `graph_walker_cycle_terminates` — A references B references A; terminates without infinite loop.
- `graph_walker_where_clause_filters` — child plugin's `where` clause is false; child not loaded.
- `graph_walker_transitive_skills_installed` — after sync, skills from both A and transitive B are in `.claude/skills/`.

### Provenance

- `provenance_used_from_config` — config-sourced plugins get `used` provenance.
- `provenance_workspace_from_cwd` — workspace root plugin gets `workspace` provenance.
- `provenance_transitive_inheritance` — if A (`used`) loads B, B inherits `used`.
- `provenance_merge_on_multiple_paths` — same plugin reached from config and workspace gets both.
- `workspace_predicate_true_for_workspace_provenance` — `workspace()` evaluates true.
- `workspace_predicate_false_for_used_only` — `workspace()` false for `used`-only plugin.
- `used_predicate_true` — `used()` true for config-sourced.
- `dependency_predicate_true` — `dependency()` true for discovered deps.
- `provenance_gates_skill_activation` — plugin with `where.predicate = "workspace()"` skill: not installed when loaded via config, installed when loaded as workspace.

### Plugin commands

- `plugin_list_shows_graph` — output includes transitive plugins with provenance labels.
- `plugin_validate_rejects_removed_syntax` — `source = "crate"` in a manifest produces an error with migration hint.
