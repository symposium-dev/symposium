# OpenCode

Config name: `opencode`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

**Not supported.** OpenCode's hook system is based on TypeScript/JavaScript plugins, not shell commands. Symposium cannot register hooks for OpenCode.

Skill files are installed but `symposium hook` will never be called by this agent.
