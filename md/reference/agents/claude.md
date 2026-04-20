# Claude Code

Config name: `claude`

## Skills

| Scope | Path |
|-------|------|
| Project | `.claude/skills/<name>/SKILL.md` |
| Global | `~/.claude/skills/<name>/SKILL.md` |

Claude Code does not support the vendor-neutral `.agents/skills/` path.

## Hooks

Symposium merges hook entries into Claude Code's `settings.json`.

| Scope | File |
|-------|------|
| Project | `.claude/settings.json` |
| Global | `~/.claude/settings.json` |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart` (PascalCase).

Output format: JSON with `hookSpecificOutput` wrapper. Exit code 2 blocks tool use.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.claude/settings.json` | `mcpServers.<name>` |
| Global | `~/.claude/settings.json` | `mcpServers.<name>` |
