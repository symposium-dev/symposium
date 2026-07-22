# Dependency predicates (`depends-on`)

Dependency predicates control when plugins, skill groups, and individual skills are active. A predicate matches against a **workspace's direct dependency set** — not against individual packages in isolation. Today the dependency set is the workspace's cargo dependency graph; a `depends-on` atom matches a direct dependency by name.

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

Predicates match against **direct** workspace dependencies only, not transitive ones.

## Usage in different contexts

### Plugin manifests (TOML)

The `depends-on` field accepts an array of atom strings:

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

## Migration from `crates`

`depends-on` replaces the former `crates` field and `crate(...)` predicate (renamed as part of the [registry-centric plugin distribution RFD](../rfds/registry-centric-plugins/README.md), which generalizes dependency matching beyond cargo). The old spellings are rejected at parse time with a migration hint — the atom syntax itself is unchanged, so migrating is a mechanical rename.
