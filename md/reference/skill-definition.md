# Skill definitions

A skill is a `SKILL.md` file inside a skill directory. Skills follow the [agentskills.io](https://agentskills.io/specification.md) format.

Skills can be supplied by a [plugin](./plugin-definition.md) or [by adding skills into the `.agents/skills` directory within the workspace](../workspace-skills.md).

## Directory layout

```text
skills/
  my-skill/
    SKILL.md
    scripts/       # optional
    resources/     # optional
```

## Symlinks in skill content

A skill directory may contain symlinks, and installing a skill **follows** them:
a link to a file arrives as a real file, a link to a directory arrives as a real
directory holding a copy of its contents. That is what lets a workspace keep one
copy of a shared script and link it into the several skills that use it.

Links are followed wherever they point, including outside the skill directory
and outside the crate or registry entry that ships it. Two alternatives were
considered and rejected:

- **Copying the link itself.** The installed skill lives in the agent's skill
  directory, a different tree, so a relative link pointing outside the skill
  directory dangles there — which is exactly the missing-file error this is
  meant to avoid.
- **Following only links that stay inside the source.** This breaks layouts that
  already exist, such as a skill linking a library out of a sibling crate in the
  same workspace, and it would make installing from a source tree stricter than
  installing the same crate from crates.io: `cargo package` follows links,
  including ones whose target escapes the package root, so the published crate
  carries that content as a real file either way.

The consequence is worth stating plainly: a skill you install can bring any file
its links reach into the directory your agent reads. The boundary that governs
that is [enablement](./configuration.md#plugins) — a plugin embedded in a
dependency does not run without your consent, and a registry is a trust root you
added yourself. Symposium does not put a second, filesystem-shaped boundary on
top of it.

Two links are skipped rather than followed, each with a warning, and neither
fails the sync:

- a **broken** link, whose target does not exist;
- a link naming a **directory the copy is already inside**, which would
  otherwise nest forever.

## SKILL.md format

A `SKILL.md` file has YAML frontmatter followed by a markdown body:

```markdown
---
name: serde-basics
description: Basic guidance for serde usage
depends-on: serde
---

Prefer deriving `Serialize` and `Deserialize` on data types.
```

## Frontmatter fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Skill identifier. |
| `description` | string | yes | Short description shown in skill listings. |
| `depends-on` | string | no | Comma-separated dependency atoms this skill is about (e.g., `depends-on: serde, tokio>=1.0`). Narrows the enclosing `[[skills]]` group scope — cannot widen it. |
| `predicates` | string | no | Comma-separated predicates (`depends-on`, `shell`, `path_exists`, `env`, `workspace-member`, `not`, `any`, `all`); all must hold for the skill to activate. ANDed with plugin- and group-level predicates. See [Predicates](./predicates.md). |

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

See [Crate predicates](./depends-on.md) for the full syntax.

## Scope composition

`depends-on` can be declared at the `[[skills]]` group level (in the plugin TOML) and at the individual skill level (in SKILL.md frontmatter). They compose as AND: both layers must match for a skill to activate. A skill-level `depends-on` narrows the group's scope — it does not widen it.
