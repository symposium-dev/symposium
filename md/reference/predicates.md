# Predicates

A **predicate** is a function-call expression that decides whether a plugin, skill group, skill, hook, or MCP server should be active based on the live environment. Predicates are declared in the `predicates` field and complement the crate-graph [crate predicates](./crate-predicates.md) (`crates`).

The available predicate functions are:

| Predicate | Holds when |
|-----------|------------|
| `shell(<command>)` | `<command>` run via `sh -c` exits `0`. Any other exit (including spawn failure) fails. |
| `path_exists(<arg>)` | `<arg>` resolves to an existing path. An argument with a path separator is checked on the filesystem (cwd-relative or absolute). A bare name with no separator is checked against the cwd and then searched on `$PATH`, so it matches either a local entry (`path_exists(.git)`) or an installed binary (`path_exists(rg)`). |
| `env(<name>)` | The environment variable `<name>` is set (to any value). |
| `env(<name>=<value>)` | `<name>` is set and equals `<value>` exactly. Only the first `=` separates name from value, so `env(KEY=a=b)` matches the value `a=b`. |
| `not(<predicate>)` | The inner predicate does **not** hold. The only way to express absence. |
| `any(<p>, <p>, …)` | At least one inner predicate holds (logical **OR**). |

Predicates compose with **AND** semantics within a list: every entry must hold. They compose with **AND** across levels too, alongside `crates`. Both kinds can be set independently. `any(...)` gives OR within a single entry and `not(...)` gives negation, so the three combine into full boolean logic.

The argument of a leaf predicate (`shell`, `path_exists`, `env`) is taken **verbatim** between the parentheses — it is *not* quoted. `shell(command -v rg)` runs `command -v rg`; do not wrap the argument in quotes (they would become part of the command). An inner `)` is fine as long as parentheses balance, so `shell(echo $(date))` works. The combinators `not(...)` and `any(...)` take nested predicates as their arguments and may be nested arbitrarily, e.g. `not(any(env(CI), path_exists(.skip)))`.

## When predicates are evaluated

Predicates are evaluated at the same point the workspace's crate predicates are evaluated for that item:

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
crates = ["*"]
predicates = ["path_exists(rg)", "shell(test -f Cargo.toml)"]

[[skills]]
crates = ["serde"]
predicates = ["path_exists(jq)"]
source = "crate"

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

Like `crates`, `predicates` is **comma-separated** on a single line in SKILL.md frontmatter. Commas inside `(...)` are not treated as separators, so a `shell(...)` command may itself contain commas:

```yaml
---
name: my-skill
description: Skill that depends on ripgrep
crates: serde
predicates: path_exists(rg), shell(test -f Cargo.toml)
---
```

## Example: gating a plugin on tool availability

```toml
name = "uses-jq"
crates = ["*"]
predicates = ["path_exists(jq)"]

[[hooks]]
name = "format-json"
event = "PreToolUse"
command = { script = "scripts/format.sh" }
```

The hook here only registers if `jq` is on the user's `$PATH`. No error, no warning — symposium just skips this plugin's contributions while `jq` is missing.

## Combining predicates

`env`, `not`, and `any` cover the cases plain tool checks can't:

```toml
# Opt-in: only when a flag is set.
predicates = ["env(SYMPOSIUM_EXPERIMENTAL)"]

# Opt-out / escape hatch: skip when a marker file is present, or in CI.
predicates = ["not(path_exists(.skip-hooks))", "not(env(CI))"]

# Tool packaged under different names across distros.
predicates = ["any(path_exists(fd), path_exists(fdfind))"]

# Combinators nest: a fallback that activates only when the preferred tool is absent.
predicates = ["not(any(path_exists(rg), path_exists(ag)))"]
```
