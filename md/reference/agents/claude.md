# Claude Code

Config name: `claude`

## Skills

| Scope | Path |
|-------|------|
| Project | `.claude/skills/<name>/SKILL.md` |

Claude Code does not support the vendor-neutral `.agents/skills/` path.

## Hooks

Symposium merges hook entries into Claude Code's `settings.json`.

| Scope | File |
|-------|------|
| Project | `.claude/settings.json` |
| Global | `~/.claude/settings.json` (`$CLAUDE_CONFIG_DIR/settings.json` when set) |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart`, `Stop` (PascalCase).

Output format: JSON with `hookSpecificOutput` wrapper. Exit code 2 blocks tool use.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.mcp.json` | `mcpServers.<name>` |
| Global | `~/.claude.json` (`$CLAUDE_CONFIG_DIR/.claude.json` when set) | `mcpServers.<name>` |

MCP servers do not go in `settings.json`, which holds only the hooks.
