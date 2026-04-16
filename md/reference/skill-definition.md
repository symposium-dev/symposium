# Skill definition reference

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
activation: always
---

Prefer deriving `Serialize` and `Deserialize` on data types.
```

## Frontmatter fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Skill identifier. |
| `description` | string | yes | Short description shown in skill listings. |
| `crates` | string | no | Comma-separated crate atoms this skill is about (e.g., `crates: serde, tokio>=1.0`). Narrows the enclosing `[[skills]]` group scope — cannot widen it. |
| `activation` | string | no | `always` or `optional`. Defaults to `optional`. |

## Crate atoms

Crate atoms specify a crate name with an optional version constraint:

- `serde` — any version
- `tokio>=1.40` — 1.40 or newer
- `regex<2.0` — below 2.0
- `serde=1.0` — compatible-with-1.0 (equivalent to `^1.0`)
- `serde==1.0.219` — exact version

See [Skill matching](./skill-matching.md) for the full syntax.

## Activation modes

| Mode | Behavior |
|------|----------|
| `always` | Skill body is inlined in `cargo agents crate` output. Use for guidance that's broadly relevant whenever the crate is in use. |
| `optional` (default) | Skill is listed with metadata and path but body is not inlined. Use for targeted workflows, migration guides, or debugging aids. |

## Scope composition

`crates` can be declared at the `[[skills]]` group level (in the plugin TOML) and at the individual skill level (in SKILL.md frontmatter). They compose as AND: both layers must match for a skill to activate. A skill-level `crates` narrows the group's scope — it does not widen it.
