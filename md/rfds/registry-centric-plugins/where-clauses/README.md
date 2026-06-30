# Where clause syntax

## TL;DR

Move `crates` and `predicates` from bare top-level fields to a `where` table on all gated constructs. Rename `crates`/`crate` to `cargo` to align with the registry naming convention.

## Change in a nutshell

Previously, `crates` and `predicates` were bare top-level fields on plugins, skill groups, etc. We move them under a `where` table and rename the crate-specific fields:

| Before | After |
|--------|-------|
| `crates = ["serde"]` | `where.cargo = { serde = "*" }` |
| `predicates = ["env(CI)"]` | `where.predicates = ["env(CI)"]` |

**Why the namespace?** Registry-specific nouns like `cargo` will have siblings (`where.npm`, `where.pypi`). By scoping them under `where`, they can't collide with other top-level keys we might add (e.g., `name`, `source`, `defaults`). The rule is: anything that names a registry-specific package always lives under a table, never bare.

**Why `cargo` instead of `crate`/`crates`?** The key names the *ecosystem* (matching `npm`, `pypi`, etc.), not the unit. This is consistent with how `source.cargo` names the registry, not the artifact type.

### Fields

The `where` clause accepts:

| Field | Example value | Description |
| --- | --- | --- |
| `predicate` | `"workspace()"` | A single predicate to evaluate. |
| `predicates` | `["workspace()", "env(CI)"]` | A list of predicates to evaluate, all of which must be true. |
| `cargo` | `{ serde = ">=1.0", tokio = "*" }` | Cargo packages which must be present in the workspace. |

Each registry plugin adds its own key under `where` (e.g., `where.npm`, `where.pypi`). The values are opaque to Symposium — routed to the registry for evaluation.

`predicate`/`predicates` is builtin (always available). Registry keys like `cargo` are provided by the corresponding registry plugin (initially built-in for cargo).

All fields are ANDed together. `where.cargo = { serde = "*" }` is sugar for a `cargo(serde)` predicate in the function-call syntax.

### Scope

The `where` clause applies uniformly everywhere activation gating is needed:

- Plugin-level (top of `SYMPOSIUM.toml`)
- `[[plugins]]` entries (both in config and manifests)
- `[[skills]]` entries
- `[[hooks]]` entries
- `[[mcp_servers]]` entries
- `[[subcommands]]` entries

All of these previously had bare `crates` and `predicates` fields; both move under `where`.

## Removed syntax

| Removed | Replacement | Rationale |
|---------|-------------|-----------|
| `crates = [...]` (bare top-level) | `where.cargo = { ... }` | Moved under `where`, renamed to match ecosystem. |
| `crate = "foo"` (bare singular) | `where.cargo = { foo = "*" }` | Same. |
| `predicates = [...]` (bare top-level) | `where.predicates = [...]` | Same namespacing rationale. |

## Implementation plan

### Step 1: Add `where` parsing alongside bare fields

Accept both `where.cargo`/`where.predicates` and bare `crates`/`predicates` during a transition period. Emit a deprecation warning for the bare form.

### Step 2: Migrate all fixtures and documentation

Update test fixtures, examples, and reference docs to use `where.*` syntax.

### Step 3: Remove bare field support

Remove the bare `crates`/`predicates` deserialization paths. Bare fields in user manifests produce a parse error with a migration hint.

## Tests

### Parsing

- `where_cargo_table_parses` — `where.cargo = { serde = "*" }` deserializes correctly.
- `where_cargo_with_version_constraint` — `where.cargo = { serde = ">=1.0" }` preserves constraint.
- `where_predicates_list_parses` — `where.predicates = ["env(CI)", "workspace()"]` deserializes.
- `where_predicate_singular_parses` — `where.predicate = "workspace()"` works.
- `where_all_fields_and_together` — `where.cargo` + `where.predicates` both present means AND semantics.
- `where_cargo_star_matches_any_version` — `{ serde = "*" }` matches any serde version present.

### Backward compatibility

- `bare_crates_parses_with_deprecation` — bare `crates = ["serde"]` still loads but emits deprecation warning.
- `bare_predicates_parses_with_deprecation` — bare `predicates = [...]` still loads with warning.
- `bare_crates_migrates_to_where_cargo` — internal representation is the same regardless of syntax form.

### Scope coverage

- `where_on_plugin_level` — top of `SYMPOSIUM.toml` with `where.cargo`.
- `where_on_plugins_entry` — `[[plugins]]` with `where.predicates`.
- `where_on_skills_entry` — `[[skills]]` with `where.cargo`.
- `where_on_hooks_entry` — `[[hooks]]` with `where.predicates`.
- `where_on_mcp_servers_entry` — `[[mcp_servers]]` with `where.cargo`.
- `where_on_subcommands_entry` — `[[subcommands]]` with `where.cargo`.

### After bare removal (Step 3)

- `bare_crates_errors_after_removal` — bare `crates = [...]` produces a parse error with migration hint.
- `bare_predicates_errors_after_removal` — same for bare `predicates`.

### Integration

- `where_cargo_gates_skill_installation` — plugin with `[[skills]] where.cargo = { tokio = "*" }`: skills not installed without tokio dep, installed with it.
- `where_predicates_gates_hook_dispatch` — hook with `where.predicates = ["env(MY_FLAG)"]`: not dispatched without env var, dispatched with it.
