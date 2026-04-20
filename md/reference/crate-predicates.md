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
