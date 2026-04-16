# Defining Skills

Skills are discovered from directories that contain `SKILL.md` files.

The expected layout is:

```text
skills/
  my-skill/
    SKILL.md
    scripts/
    resources/
```

`SKILL.md` starts with `---` frontmatter followed by a markdown body.

## Minimal example

```markdown
---
name: serde-basics
description: Basic guidance for serde usage
crates: serde
activation: always
---

Prefer deriving `Serialize` and `Deserialize` on data types.
```

## Determining when the skill applies

The `crates` field declares which crate(s) the skill is about (comma-separated in frontmatter: `crates: serde, tokio>=1.0`).

At the skill level, this field narrows the enclosing `[[skills]]` group. It does not widen it.

Examples of forms you can use today:

- `serde`: any version of `serde`
- `tokio>=1.40`: `tokio` at `1.40` or newer
- `regex<2.0`: any `regex` version below `2.0`
- `serde=1.0`: compatible-with-`1.0` matching
- `serde==1.0.219`: exact version match

See [Skill Matching Reference](./reference/skill-matching.md) for the full atom syntax.

Example:

```markdown
---
name: serde-with-regex
description: Guidance for projects combining serde and regex
crates: serde, regex
activation: always
---

Keep serialized regex patterns in a stable string form.
```

## Activation modes

Activation controls how the skill is presented when Symposium finds a match.

- `always`: inline the skill body in `cargo agents crate` output
- `optional`: list the skill and its path without inlining the body

Use `always` for broad usage guidance that is usually relevant whenever the crate is in use.

Use `optional` for targeted workflows or checks that are only sometimes needed, such as migration notes, debugging steps, or one-off integration tasks.
