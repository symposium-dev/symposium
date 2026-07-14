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

## Matched crate set

When a skill group uses `source = "crate"`, predicates serve a second purpose beyond filtering: they determine **which crate sources to fetch**.

Each non-wildcard atom that matches a workspace dependency contributes that dependency's name and version to the *matched crate set*. Symposium then fetches the source for each crate in the set and looks for skills inside it.

- `"serde"` against a workspace with `serde 1.0.210` → matched set: `{serde@1.0.210}`
- `"serde"` against a workspace without serde → no match, plugin skipped
- `"*"` → matches, but contributes no concrete crates to the set
- `["serde", "tokio"]` with both in workspace → `{serde@1.0.210, tokio@1.38.0}`

Predicates from both the plugin level and the group level are unioned together to form the matched set.

Because wildcards contribute no concrete crates, **at least one non-wildcard atom must be present** (at either the plugin or group level) when using crate-sourced skills. A manifest with only wildcards and `source = "crate"` is rejected at parse time.

When the gate uses combinators (`any`, `all`, `not`) or mixes `depends-on(...)` with other predicates, the matched set generalizes to the predicate's **witness** — the concrete crates that participate in a satisfying evaluation. See [Crate-sourced skills and the witness](./predicates.md#crate-sourced-skills-and-the-witness).

## Migration from `crates`

`depends-on` replaces the former `crates` field and `crate(...)` predicate (renamed as part of the [registry-centric plugin distribution RFD](../rfds/registry-centric-plugins/README.md), which generalizes dependency matching beyond cargo). The old spellings are rejected at parse time with a migration hint — the atom syntax itself is unchanged, so migrating is a mechanical rename.
