# Workspace skills

In addition to adding skills based on your dependencies, Symposium will also install skills your workspace defines for itself, and copy any additional skills found in `.agents/skills` into the directory appropriate for your configured agent(s).

This "skill-syncing" feature allows your project to add skills in one central location that will work for all developers, regardless of which agent they use (for example, Claude Code users will have the skills synced to `.claude/skills`).

The default skill location therefore varies depending on the intended audience:

| Skills intended for... | Go into... |
| --- | --- |
| Maintaining your crate | `.agents/skills` |
| [Using your crate](./crate-authors/supporting-your-crate.md) | `skills/` |

## Workspace plugins

The workspace root and every member crate directory can define a *workspace plugin*: add a `SYMPOSIUM.toml` manifest (see the [plugin definition](./reference/plugin-definition.md)), or just a bare `skills/` directory — a directory with skills and no manifest counts as a plugin whose only content is those skills.

Workspace plugins are always active while you work in that workspace — no plugin source configuration or `depends-on` gate is needed. A `skills/` directory in a member crate serves double duty: it installs for everyone working in the workspace *and*, once published, for projects that depend on the crate.

Two details specific to workspace manifests:

- `name` may be omitted; it defaults to the directory name.
- The default `skills/` group can be disabled with `[defaults] skills = false`.

Components that should apply only to people developing the workspace (not to dependents of a published crate) can be gated with the [`workspace-member()` predicate](./reference/predicates.md).

## Recommended git setup

We recommend you commit your `.agents/skills` or `skills/` into the repository. Symposium installs a `.gitignore` file into every skill that it creates, so automatically copied and installed skills should not dirty your git status.

## Pre-existing files

Symposium never touches skills in `.claude/skills/`, `.kiro/skills/` etc. that it did not put there itself. If you previously hand-wrote a skill with the same name as one in `.agents/skills/`, propagation is skipped for that name and a warning is printed — your existing file stays in place.
