# Goose

Config name: `goose`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

**Not supported.** Goose has no hook system. It uses MCP extensions for extensibility.

Skill files are installed but `cargo agents hook` will never be called by this agent.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.goose/config.yaml` | `extensions.<name>` |
| Global | `~/.config/goose/config.yaml` | `extensions.<name>` |
