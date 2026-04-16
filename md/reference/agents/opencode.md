# OpenCode

Config name: `opencode`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

**OpenCode does not support shell-command hooks.** Its extensibility is based on TypeScript/JavaScript plugins. Symposium cannot register hooks for OpenCode.

OpenCode is supported as a skills-only agent — `cargo agents sync` will install skill files, but no hooks are registered.
