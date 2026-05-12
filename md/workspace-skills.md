# Workspace skills

You can author your own skills directly inside a Rust workspace without
publishing them to a plugin repository. Drop them into
`.agents/skills/<skill-name>/` and `cargo agents sync` will make them
available to every agent you use.

## Why `.agents/skills/`

Different agents read skills from different locations. Copilot, Gemini,
Codex, Goose, and OpenCode all look in the vendor-neutral `.agents/skills/`
directory; Claude Code reads `.claude/skills/`; Kiro reads `.kiro/skills/`.

Symposium treats `.agents/skills/` as the canonical place for
*workspace-local, user-authored* skills. When [`cargo agents sync`]
runs, any skill you put there is mirrored into the directories used by
your other configured agents, so you only have to write the skill once.

[`cargo agents sync`]: ./reference/cargo-agents-sync.md

## Authoring a skill

Create a directory named after your skill, with a `SKILL.md` following
the [skill definition] format:

```text
<workspace-root>/
  .agents/
    skills/
      our-coding-style/
        SKILL.md
        style-guide.md   # optional companion files are copied too
```

A minimal `SKILL.md`:

```markdown
---
name: our-coding-style
description: Team coding conventions for this repository.
---

Prefer `tracing::info!` over `println!`. See `style-guide.md` for the
full guide.
```

Any sibling files inside the skill directory (references, scripts,
examples) are copied along with `SKILL.md`.

[skill definition]: ./reference/skill-definition.md

## What sync does

Running `cargo agents sync` will:

1. Leave `.agents/skills/our-coding-style/` untouched — that's your
   working copy.
2. Copy the skill into each configured agent's own skill directory
   when it differs from `.agents/skills/`. For example, with Claude
   and Kiro configured you'll get `.claude/skills/our-coding-style/`
   and `.kiro/skills/our-coding-style/`.
3. Record the propagated copies in each destination's
   `.symposium.toml` manifest under a `propagated` list, so they can
   be cleaned up later.

If the only agents you use already read from `.agents/skills/` (e.g.,
only Copilot or Codex), nothing extra needs to happen — the source
directory already is every agent's skill directory.

## Updating and removing skills

- **Edit a skill** — edit the files under `.agents/skills/<name>/` and
  run `cargo agents sync`. The propagated copies are overwritten with
  your new content.
- **Remove a skill** — delete `.agents/skills/<name>/` and run
  `cargo agents sync`. The propagated copies are removed from the
  other agent directories.
- **Disable the feature** — set `agents-syncing = false` in
  `~/.symposium/config.toml` (see [Configuration]). On the next sync,
  previously propagated copies are removed as well.

[Configuration]: ./reference/configuration.md

## Workspace skills vs. plugins

| Use a workspace skill when… | Use a plugin when… |
|-----------------------------|--------------------|
| The skill is specific to *this* repository. | The skill applies to any project depending on a given crate. |
| You're iterating on the content and want it versioned alongside the code. | You want to share the skill with other repositories or organizations. |
| You don't want to publish a skill source. | You're happy to publish to a git repository or the central recommendations repository. |

For cross-project or cross-organization distribution, see
[Custom plugin sources](./custom-plugin-source.md) and
[Authoring a plugin](./crate-authors/authoring-a-plugin.md).

## Safety notes

- Symposium never touches skills in `.claude/skills/`, `.kiro/skills/`
  etc. that it did not put there itself. If you previously hand-wrote a
  skill with the same name as one in `.agents/skills/`, propagation is
  skipped for that name and a warning is printed — your existing file
  stays in place.
- Symposium tracks what it installed vs. propagated through the
  per-directory `.symposium.toml` manifest. Deleting that manifest
  makes symposium forget what it owns, and it will stop cleaning up
  after itself.
