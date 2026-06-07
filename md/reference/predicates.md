# Predicates

A **predicate** decides whether a plugin, skill group, skill, hook, MCP server, or subcommand is active, evaluated against the workspace's crate graph and the live environment. There is one predicate model, written two ways:

- The **`crates`** field uses crate-atom syntax (see [crate predicates](./crate-predicates.md)) and is **sugar**: `crates = ["serde", "tokio"]` lowers to a single `any(crate(serde), crate(tokio))` predicate.
- The **`predicates`** field uses the function-call syntax below.

Both fields are merged into one list that is ANDed together, so `crates` and `predicates` compose with **AND**. A `crate(...)` predicate is available in `predicates` too — `crates` just makes the common case terse.

The available predicate functions are:

| Predicate | Holds when |
|-----------|------------|
| `crate(<name>)` / `crate(<name><req>)` | A workspace dependency named `<name>` is present (and its version satisfies `<req>`, e.g. `crate(serde>=1.0)`). |
| `crate(*)` | Any workspace matches (even one with zero dependencies). The lowered form of `*`. |
| `shell(<command>)` | `<command>` run via `sh -c` exits `0`. Any other exit (including spawn failure) fails. |
| `path_exists(<arg>)` | `<arg>` resolves to an existing path. An argument with a path separator is checked on the filesystem (cwd-relative or absolute). A bare name with no separator is checked against the cwd and then searched on `$PATH`, so it matches either a local entry (`path_exists(.git)`) or an installed binary (`path_exists(rg)`). |
| `env(<name>)` | The environment variable `<name>` is set (to any value). |
| `env(<name>=<value>)` | `<name>` is set and equals `<value>` exactly. Only the first `=` separates name from value, so `env(KEY=a=b)` matches the value `a=b`. |
| `not(<predicate>)` | The inner predicate does **not** hold. The only way to express absence. |
| `any(<p>, <p>, …)` | At least one inner predicate holds (logical **OR**). |
| `all(<p>, <p>, …)` | Every inner predicate holds (logical **AND**). |

Predicates compose with **AND** semantics within a list: every entry must hold. `any(...)` gives OR within a single entry, `all(...)` gives an explicit AND group, and `not(...)` gives negation — together they form full boolean logic. They also compose with **AND** across levels (plugin ∧ group ∧ skill).

The argument of a leaf predicate (`crate`, `shell`, `path_exists`, `env`) is taken **verbatim** between the parentheses — it is *not* quoted. `shell(command -v rg)` runs `command -v rg`; do not wrap the argument in quotes (they would become part of the command). An inner `)` is fine as long as parentheses balance, so `shell(echo $(date))` works. The combinators `not`, `any`, and `all` take nested predicates as their arguments and may be nested arbitrarily, e.g. `not(any(env(CI), path_exists(.skip)))`.

## Crate-sourced skills and the witness

For a `[[skills]]` group with `source = "crate"`, the `crate(...)` predicates do double duty: they gate the group **and** name which crates' source trees to fetch skills from. The fetch set is the predicate's **witness** — the crates that participate in a *satisfying* evaluation: `crate(c)` contributes `c` when present, `any` contributes its true branches, `all` contributes all branches when it holds, and `not(...)` contributes nothing. So `all(crate(serde), env(USE_SERDE))` only fetches `serde` when `USE_SERDE` is set, while `any(crate(fd), crate(fdfind))` fetches whichever are present. A group using `source = "crate"` must name at least one concrete crate somewhere (plugin- or group-level); `crate(*)` alone is rejected since there is nothing concrete to fetch.

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

`crate`, `env`, `not`, `any`, and `all` cover the cases plain `crates` lists can't:

```toml
# Opt-in: only when a flag is set.
predicates = ["env(SYMPOSIUM_EXPERIMENTAL)"]

# Opt-out / escape hatch: skip when a marker file is present, or in CI.
predicates = ["not(path_exists(.skip-hooks))", "not(env(CI))"]

# Tool packaged under different names across distros.
predicates = ["any(path_exists(fd), path_exists(fdfind))"]

# A crate gate that also requires an env flag (vs. the bare `crates = ["serde"]`).
predicates = ["all(crate(serde), env(USE_SERDE))"]

# Apply only when a crate is absent (impossible with `crates`).
predicates = ["not(crate(legacy-thing))"]
```

These are equivalent — `crates` is just the terse form for the common case:

```toml
crates = ["serde", "tokio"]
# is exactly
predicates = ["any(crate(serde), crate(tokio))"]
```
