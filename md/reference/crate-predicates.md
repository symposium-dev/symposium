# Crate predicates

Crate predicates control when plugins, skill groups, and individual skills are active. A predicate matches against a **workspace's direct dependency set** — not against individual crates in isolation.

## Predicate syntax

A crate predicate is a crate name with an optional version requirement.

Examples:

- `serde`
- `serde>=1.0`
- `tokio^1.40`
- `regex<2.0`
- `serde=1.0`
- `serde==1.0.219`
- `*`

Semantics:

- bare crate name: matches if the workspace has this crate as a direct dependency (any version)
- `>=`, `<=`, `>`, `<`, `^`, `~`: standard semver operators applied to the workspace's version of the crate
- `=1.0`: compatible-version matching, equivalent to `^1.0`
- `==1.0.219`: exact-version matching
- `*`: wildcard — always matches, even a workspace with zero dependencies

Predicates match against **direct** workspace dependencies only, not transitive ones.

## Usage in different contexts

### Plugin manifests (TOML)

The `crates` field accepts an array of predicate strings:

- `crates = ["serde"]`
- `crates = ["serde", "tokio>=1.40"]`
- `crates = ["*"]` (wildcard — always active)

### Skill frontmatter (YAML)

The `crates` field uses comma-separated values:

- `crates: serde`
- `crates: serde, tokio>=1.40`

## Matching behavior

A `crates` list matches if *at least one* predicate in the list matches the workspace. The wildcard `*` always matches — even a workspace with zero dependencies.

If there are multiple `crates` declarations in scope, all of them must match (AND composition). For example with skills, `crates` predicates can appear at three distinct levels:

* If a [plugin](./plugin-definition.md) defines `crates` at the top-level, it must match before any other plugin contents will be considered.
* If a skill-group within a plugin defines `crates`, that predicate must match before the skills themselves will be fetched.
* If the skills define `crates` in their front-matter, those crates must match before the skills will be added to the project.

## Matched crate set

When a skill group uses `source = "crate"` or `source.crate_path`, predicates serve a second purpose beyond filtering: they determine **which crate sources to fetch**.

Each non-wildcard predicate that matches a workspace dependency contributes that dependency's name and version to the *matched crate set*. Symposium then fetches the source for each crate in the set and looks for skills inside it.

- `"serde"` against a workspace with `serde 1.0.210` → matched set: `{serde@1.0.210}`
- `"serde"` against a workspace without serde → no match, plugin skipped
- `"*"` → matches, but contributes no concrete crates to the set
- `["serde", "tokio"]` with both in workspace → `{serde@1.0.210, tokio@1.38.0}`

Predicates from both the plugin level and the group level are unioned together to form the matched set.

Because wildcards contribute no concrete crates, **at least one non-wildcard predicate must be present** (at either the plugin or group level) when using crate-sourced skills. A manifest with only wildcards and `source = "crate"` is rejected at parse time.
