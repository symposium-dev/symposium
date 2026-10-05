# Dependency predicates (`depends-on`)

Dependency predicates control when plugins and their components are active. A predicate matches against a **workspace's direct dependency set**, not against individual packages in isolation. Today the dependency set is the workspace's cargo dependency graph; a `depends-on` atom matches a direct dependency by name.

The `depends-on` field is accepted on the plugin itself, on `[[skills]]` groups, `[[mcp_servers]]` entries, `[[plugins]]` edges, and `[subcommand.<name>]` tables (see [Plugin definitions](./plugin-definition.md)), and in [SKILL.md frontmatter](./skill-definition.md). `[[hooks]]` and `[[predicate]]` entries have no `depends-on` field; gate a hook on a dependency with `predicates = ["depends-on(serde)"]`.

The `depends-on` field is shorthand: `depends-on = ["serde", "tokio"]` lowers to a single `any(depends-on(serde), depends-on(tokio))` predicate and is merged into the same list as the [`predicates`](./predicates.md) field (ANDed together). Everything below describes the dependency-atom syntax `depends-on` accepts; the equivalent `depends-on(<atom>)` predicate is also usable directly in `predicates`.

## Predicate syntax

A dependency atom is a package name with an optional version requirement.

Examples:

- `serde`
- `serde>=1.0`
- `tokio^1.40`
- `regex<2.0`
- `serde=1.0`
- `serde==1.0.219`
- `*`

Semantics:

- bare name: matches if the workspace has this package as a direct dependency (any version)
- `>=`, `<=`, `>`, `<`, `^`, `~`: standard semver operators applied to the workspace's version of the package
- `=1.0`: compatible-version matching, equivalent to `^1.0`
- `==1.0.219`: exact-version matching
- `*`: wildcard — always matches, even a workspace with zero dependencies

> **`=` is not Cargo's `=`.** In a dependency atom, `=1.0` means `^1.0` (1.0 or any later 1.x release), which is what a bare `1.0` means in Cargo. To pin an exact version, use `==`: `serde==1.0.219` matches only 1.0.219.

Further rules:

- Write the atom without spaces. `serde >=1.0` is rejected; write `serde>=1.0`.
- Names match exactly, as the crate is published: a `serde-json` atom does not match the `serde_json` crate.
- In TOML, an atom may carry a comma-separated range, every part of which must hold: `"serde>=1.0,<2.0"`. SKILL.md frontmatter splits `depends-on` on commas, so a range cannot be written there; use `predicates: depends-on(serde>=1.0,<2.0)` instead.

Predicates match against **direct** workspace dependencies only, not transitive ones.

## Usage in different contexts

### Plugin manifests (TOML)

The `depends-on` field accepts a single atom string or an array of them:

- `depends-on = ["serde"]`
- `depends-on = ["serde", "tokio>=1.40"]`
- `depends-on = ["*"]` (wildcard — always active)

### Skill frontmatter (YAML)

The `depends-on` field uses comma-separated values:

- `depends-on: serde`
- `depends-on: serde, tokio>=1.40`

## Matching behavior

A `depends-on` list matches if *at least one* atom in the list matches the workspace. The wildcard `*` always matches — even a workspace with zero dependencies.

If there are multiple `depends-on` declarations in scope, all of them must match (AND composition). For example with skills, `depends-on` predicates can appear at three distinct levels:

* If a [plugin](./plugin-definition.md) defines `depends-on` at the top-level, it must match before any other plugin contents will be considered.
* If a skill-group within a plugin defines `depends-on`, that predicate must match before the skills themselves will be fetched.
* If the skills define `depends-on` in their front-matter, those dependencies must match before the skills will be added to the project.

`depends-on` is purely a gate — it decides *whether* an item activates, not which crate to fetch. To load a crate's own skills, name that crate explicitly in a [`[[plugins]]` chained reference](./plugin-definition.md#chained-plugins) (`source.cargo = "..."`), gating the edge with `depends-on` as usual.

The `crates` field (in manifests and in SKILL.md frontmatter) and the `crate(...)` predicate are rejected; use `depends-on`.
