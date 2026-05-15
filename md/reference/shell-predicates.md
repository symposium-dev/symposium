# Shell predicates

A **shell predicate** is a shell command that decides whether a plugin, skill group, skill, hook, or MCP server should be active. Each predicate is run via `sh -c <command>`:

- exit `0` → the predicate **holds**
- any other exit (including spawn failure) → the predicate **fails**, and the enclosing item is skipped

Shell predicates compose with **AND** semantics within a list: every entry must hold. They compose with **AND** semantics across levels too, alongside the existing [crate predicates](./crate-predicates.md). Both kinds can be set independently.

## When predicates are evaluated

Shell predicates are evaluated at the same point the workspace's crate predicates are evaluated for that item:

| Level | Evaluated |
|-------|-----------|
| Plugin `shell_predicates` | At sync (gates skills & MCP) and at every hook dispatch |
| Skill group `shell_predicates` | At sync, before any git/crates source is fetched |
| Skill frontmatter `shell_predicates` | At sync, after the skill loads |
| Hook `shell_predicates` | At hook dispatch, after the matcher passes |
| MCP server `shell_predicates` | At sync, when collecting servers to register |

Hook-level predicates run at dispatch (not sync) so they observe live state — e.g. a hook gated on `command -v jq` will silently disable itself if `jq` was uninstalled since the last sync, without forcing a re-sync.

> **Tip:** keep predicates **fast** and **side-effect free** (`command -v foo`, `test -f bar`, `test -d .git`). Plugin- and hook-level predicates fire on every hook dispatch.

## Usage

### Plugin manifests (TOML)

```toml
name = "my-plugin"
crates = ["*"]
shell_predicates = ["command -v rg", "test -f Cargo.toml"]

[[skills]]
crates = ["serde"]
shell_predicates = ["command -v jq"]
source = "crate"

[[hooks]]
name = "h"
event = "PreToolUse"
command = { script = "scripts/x.sh" }
shell_predicates = ["test -d .git"]

[[mcp_servers]]
name = "tool"
command = "/usr/local/bin/tool"
args = []
env = []
shell_predicates = ["command -v tool"]
```

### Skill frontmatter (YAML)

Like `crates`, `shell_predicates` is **comma-separated** on a single line in SKILL.md frontmatter:

```yaml
---
name: my-skill
description: Skill that depends on ripgrep
crates: serde
shell_predicates: command -v rg, test -f Cargo.toml
---
```

If you need commas inside a single command, declare the skill via a plugin manifest instead — the TOML array form supports arbitrary strings.

## Example: gating a plugin on tool availability

```toml
name = "uses-jq"
crates = ["*"]
shell_predicates = ["command -v jq"]

[[hooks]]
name = "format-json"
event = "PreToolUse"
command = { script = "scripts/format.sh" }
```

The hook here only registers if `jq` is on the user's `$PATH`. No error, no warning — symposium just skips this plugin's contributions while `jq` is missing.
