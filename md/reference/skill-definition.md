# Skill definitions

A skill is a `SKILL.md` file inside a skill directory. Skills follow the [agentskills.io](https://agentskills.io/specification.md) format.

## Directory layout

```text
skills/
  my-skill/
    SKILL.md
    scripts/       # optional
    resources/     # optional
```

## SKILL.md format

A `SKILL.md` file has YAML frontmatter followed by a markdown body:

```markdown
---
name: serde-basics
description: Basic guidance for serde usage
crates: serde
---

Prefer deriving `Serialize` and `Deserialize` on data types.
```

## Frontmatter fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Skill identifier. |
| `description` | string | yes | Short description shown in skill listings. |
| `crates` | string | no | Comma-separated crate atoms this skill is about (e.g., `crates: serde, tokio>=1.0`). Narrows the enclosing `[[skills]]` group scope — cannot widen it. |

## Crate atoms

Crate atoms specify a crate name with an optional version constraint:

- `serde` — any version
- `tokio>=1.40` — 1.40 or newer
- `tokio>1.40` — strictly above 1.40
- `regex<2.0` — below 2.0
- `regex<=2.0` — 2.0 or below
- `serde^1.0` — compatible with 1.0 (same as `=1.0`)
- `serde~1.2` — patch-level changes only (>=1.2.0, <1.3.0)
- `serde=1.0` — compatible-with-1.0 (equivalent to `^1.0`)
- `serde==1.0.219` — exact version

See [Crate predicates](./crate-predicates.md) for the full syntax.

## Scope composition

`crates` can be declared at the `[[skills]]` group level (in the plugin TOML) and at the individual skill level (in SKILL.md frontmatter). They compose as AND: both layers must match for a skill to activate. A skill-level `crates` narrows the group's scope — it does not widen it.
