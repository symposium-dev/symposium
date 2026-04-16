# Publishing skills

Skills are guidance documents that teach AI assistants how to use your crate. When a user's project depends on your crate, Symposium loads your skills automatically.

## Writing a SKILL.md

A skill is a `SKILL.md` file with a few lines of YAML frontmatter followed by a markdown body:

```markdown
---
name: widgetlib-basics
description: Basic guidance for widgetlib usage
crates: widgetlib
activation: always
---

Prefer using `Widget::builder()` over constructing widgets directly.
Always call `.validate()` before passing widgets to the runtime.
```

The frontmatter tells Symposium when to load the skill. The markdown body is what the AI assistant sees — write it as direct guidance about what to do, what to avoid, and which patterns work.

## Publishing your skill

Currently publishing skills requires a PR to the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository; once the system stabilizes more, we expect to allow you to embed skills directly in your crate with no central repository.

You can either upload the skill directly to the central repo or you can upload a plugin that points to skills in your own repository.

### Uploading a single skill to the central repo

Add a directory for your crate containing a `SKILL.md`:

```text
widgetlib/
  SKILL.md
  scripts/         # optional
  resources/       # optional
```

### Uploading multiple skills to the central repo

If you have several skills, add a subdirectory for each one:

```text
widgetlib/
  basics/
    SKILL.md
  advanced-patterns/
    SKILL.md
    scripts/       # optional
    resources/     # optional
```

### Hosting skills in your own repository

If you'd rather keep skills in your crate's repository, you can add a plugin manifest to the recommendations repo that points to them and symposium will download the skills directly from your repository to users: 

```text
widgetlib.toml     # create this
```

where `widgetlib.toml` looks someting like this:

```toml
name = "widgetlib"

[[skills]]
crates = ["widgetlib"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/skills"
```

See [Creating a plugin](./creating-a-plugin.md) for more on plugin manifests.

## Frontmatter fields

| Field | Description |
|-------|-------------|
| `name` | Skill identifier. |
| `description` | Short description shown in skill listings. |
| `crates` | Which crate(s) this skill is about. Comma-separated: `crates: serde, serde_json`. |
| `activation` | `always` (inline the body) or `optional` (list but don't inline). Defaults to `optional`. |
| `compatibility` | List of agents or editors this skill works with, if it doesn't apply universally. See the [compatibility field spec](https://agentskills.io/specification#compatibility-field). |

See the [Skill definition reference](../reference/skill-definition.md) for the full format, the [Skill matching reference](../reference/skill-matching.md) for version constraint syntax, and the [agentskills.io quickstart](https://agentskills.io/skill-creation/quickstart) for general guidance on writing effective skills.

## Activation modes

- **`always`** — the skill body is included inline whenever the crate matches. Use this for guidance that's broadly relevant.
- **`optional`** (the default) — the skill is listed with its metadata but the body isn't inlined. Use this for targeted workflows, migration guides, or debugging aids that are only sometimes needed.

## Testing your skills

From a project that depends on your crate:

```bash
cargo agents crate --list
cargo agents crate widgetlib
```
