# OpenCode

Config name: `opencode`

## Skills

| Scope | Path |
|-------|------|
| Project | `.opencode/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

OpenCode also reads `.agents/skills/`, so skills you author there reach it without a copy.

OpenCode also loads the skills of a plugin installed for Claude Code with `cargo agents use --global`, in every folder and whether or not OpenCode is among your agents; set `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1` to keep it out of Claude Code's skills.

## Hooks

**OpenCode does not support shell-command hooks.** Its extensibility is based on TypeScript/JavaScript plugins. Symposium cannot register hooks for OpenCode.

OpenCode is supported as a skills-only agent — `cargo agents sync` will install skill files, but no hooks are registered.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `opencode.json` | `mcp.<name>` |
| Global | `~/.config/opencode/opencode.json` | `mcp.<name>` |
