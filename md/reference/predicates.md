# Predicates

A **predicate** decides whether a plugin, skill group, skill, hook, MCP server, or subcommand is active, evaluated against the workspace's dependency graph and the live environment. There is one predicate model, written two ways:

- The **`depends-on`** field uses dependency-atom syntax (see [dependency predicates](./depends-on.md)) and is **sugar**: `depends-on = ["serde", "tokio"]` lowers to a single `any(depends-on(serde), depends-on(tokio))` predicate.
- The **`predicates`** field uses the function-call syntax below.

Both fields are merged into one list that is ANDed together, so `depends-on` and `predicates` compose with **AND**. A `depends-on(...)` predicate is available in `predicates` too — the field just makes the common case terse.

The available predicate functions are:

| Predicate | Holds when |
|-----------|------------|
| `depends-on(<name>)` / `depends-on(<name><req>)` | A workspace dependency named `<name>` is present (and its version satisfies `<req>`, e.g. `depends-on(serde>=1.0)`). |
| `depends-on(*)` | Any workspace matches (even one with zero dependencies). The lowered form of `*`. |
| `shell(<command>)` | `<command>` run via `sh -c` exits `0`. Any other exit (including spawn failure) fails. |
| `path_exists(<arg>)` | `<arg>` resolves to an existing path. An argument with a path separator is checked on the filesystem (cwd-relative or absolute). A bare name with no separator is checked against the cwd and then searched on `$PATH`, so it matches either a local entry (`path_exists(.git)`) or an installed binary (`path_exists(rg)`). |
| `env(<name>)` | The environment variable `<name>` is set (to any value). |
| `env(<name>=<value>)` | `<name>` is set and equals `<value>` exactly. Only the first `=` separates name from value, so `env(KEY=a=b)` matches the value `a=b`. |
| `workspace-member()` | The plugin this predicate belongs to is defined by a member of the active workspace (a [workspace plugin](../workspace-skills.md)). Takes no argument. |
| `not(<predicate>)` | The inner predicate does **not** hold. The only way to express absence. |
| `any(<p>, <p>, …)` | At least one inner predicate holds (logical **OR**). |
| `all(<p>, <p>, …)` | Every inner predicate holds (logical **AND**). |

Predicates compose with **AND** semantics within a list: every entry must hold. `any(...)` gives OR within a single entry, `all(...)` gives an explicit AND group, and `not(...)` gives negation — together they form full boolean logic. They also compose with **AND** across levels (plugin ∧ group ∧ skill).

The argument of a leaf predicate (`depends-on`, `shell`, `path_exists`, `env`) is taken **verbatim** between the parentheses — it is *not* quoted. `shell(command -v rg)` runs `command -v rg`; do not wrap the argument in quotes (they would become part of the command). An inner `)` is fine as long as parentheses balance, so `shell(echo $(date))` works. The combinators `not`, `any`, and `all` take nested predicates as their arguments and may be nested arbitrarily, e.g. `not(any(env(CI), path_exists(.skip)))`.

> `crate(...)` is the retired spelling of `depends-on(...)` and is rejected at parse time with a migration hint.

## Loading a crate's skills

A predicate is purely a boolean gate — it decides *whether* an item activates, not *which* crate to fetch. To load a crate's own skills, name that crate in a [`[[plugins]]` chained reference](./plugin-definition.md#chained-plugins) (`source.cargo = "..."`) and gate the edge as you like:

```toml
[[plugins]]
depends-on = ["serde"]      # only when serde is a dependency
source.cargo = "serde"      # load serde's plugin (its skills)
```

## When predicates are evaluated

Predicates are evaluated at the same point the workspace's dependency predicates are evaluated for that item:

| Level | Evaluated |
|-------|-----------|
| Plugin `predicates` | At sync (gates skills & MCP) and at every hook dispatch |
| Skill group `predicates` | At sync, before any git/crates source is fetched |
| Skill frontmatter `predicates` | At sync, after the skill loads |
| Hook `predicates` | At hook dispatch, after the matcher passes |
| MCP server `predicates` | At sync, when collecting servers to register |

Hook-level predicates run at dispatch (not sync) so they observe live state — e.g. a hook gated on `path_exists(jq)` will silently disable itself if `jq` was uninstalled since the last sync, without forcing a re-sync.

> **Tip:** keep predicates **fast** and **side-effect free** (`path_exists(rg)`, `path_exists(.git)`, `shell(test -f Cargo.toml)`). Plugin- and hook-level predicates fire on every hook dispatch.

## Usage

### Plugin manifests (TOML)

```toml
name = "my-plugin"
depends-on = ["*"]
predicates = ["path_exists(rg)", "shell(test -f Cargo.toml)"]

[[skills]]
depends-on = ["serde"]
predicates = ["path_exists(jq)"]
source.path = "skills"

[[hooks]]
name = "h"
event = "PreToolUse"
command = { script = "scripts/x.sh" }
predicates = ["path_exists(.git)"]

[[mcp_servers]]
name = "tool"
command = "/usr/local/bin/tool"
args = []
env = []
predicates = ["path_exists(tool)"]
```

### Skill frontmatter (YAML)

Like `depends-on`, `predicates` is **comma-separated** on a single line in SKILL.md frontmatter. Commas inside `(...)` are not treated as separators, so a `shell(...)` command may itself contain commas:

```yaml
---
name: my-skill
description: Skill that depends on ripgrep
depends-on: serde
predicates: path_exists(rg), shell(test -f Cargo.toml)
---
```

## Example: gating a plugin on tool availability

```toml
name = "uses-jq"
depends-on = ["*"]
predicates = ["path_exists(jq)"]

[[hooks]]
name = "format-json"
event = "PreToolUse"
command = { script = "scripts/format.sh" }
```

The hook here only registers if `jq` is on the user's `$PATH`. No error, no warning — symposium just skips this plugin's contributions while `jq` is missing.

## Combining predicates

`depends-on`, `env`, `not`, `any`, and `all` cover the cases plain `depends-on` lists can't:

```toml
# Opt-in: only when a flag is set.
predicates = ["env(SYMPOSIUM_EXPERIMENTAL)"]

# Opt-out / escape hatch: skip when a marker file is present, or in CI.
predicates = ["not(path_exists(.skip-hooks))", "not(env(CI))"]

# Tool packaged under different names across distros.
predicates = ["any(path_exists(fd), path_exists(fdfind))"]

# A dependency gate that also requires an env flag (vs. the bare `depends-on = ["serde"]`).
predicates = ["all(depends-on(serde), env(USE_SERDE))"]

# Apply only when a dependency is absent (impossible with `depends-on`).
predicates = ["not(depends-on(legacy-thing))"]
```

These are equivalent — `depends-on` is just the terse form for the common case:

```toml
depends-on = ["serde", "tokio"]
# is exactly
predicates = ["any(depends-on(serde), depends-on(tokio))"]
```
