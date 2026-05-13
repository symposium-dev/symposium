# Workspace skills

In addition to adding skills based on your dependencies, Symposium will also copy any additional skills found in `.agents/skills` into the directory appropriate for your configured agent(s).

This "skill-syncing" feature allows your project to add skills in one central location that will work for all developers, regardless of which agent they use (for example, Claude Code users will have the skills synced to `.claude/skills`).

The default skill location therefore varies depending on the intended audience:

| Skills intended for... | Go into... |
| --- | --- |
| Maintaining your crate | `.agents/skills` |
| [Using your crate](./crate-authors/supporting-your-crate.md) | `.symposium/skills` |

## Setting the "distribution" for `.symposium` skills

Some projects do not like to check-in the `.agents` directory. An alternative is to add skills into `.symposium/skills` and set the `distribution` header to `workspace`:

```
# Example
#
# In .symposium/skills/integration-test/SKILL.md

---
name: Adding an integration test
description: "Instructions for adding integration tests into this project"
distribution: workspace
---

...
```

## Recommended git setup

We recommend you commit your `.agents/skills` or `.symposium/skills` into the repository. Symposium installs a `.gitignore` file into every skill that it creates, so automatically copied and installed skills should not dirty your git status.

## Pre-existing files

Symposium never touches skills in `.claude/skills/`, `.kiro/skills/` etc. that it did not put there itself. If you previously hand-wrote a skill with the same name as one in `.agents/skills/`, propagation is skipped for that name and a warning is printed — your existing file stays in place.
