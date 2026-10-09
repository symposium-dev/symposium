# Claude Code

Config name: `claude`

## Skills

| Scope | Path |
|-------|------|
| Project | `.claude/skills/<name>/SKILL.md` |

Claude Code does not support the vendor-neutral `.agents/skills/` path.

## Plugins

A plugin enabled with `cargo agents use --global` is installed for you as a plugin directory:

| Scope | Path |
|-------|------|
| Global | `~/.claude/skills/<plugin>/` (`$CLAUDE_CONFIG_DIR/skills/<plugin>/` when set) |

Claude Code loads it in every project as `<plugin>@skills-dir`, and its skills are invoked as `/<plugin>:<skill>`. Symposium writes no settings for it; `claude plugin disable <plugin>@skills-dir` turns it off, and symposium leaves that choice alone. A plugin installed while a session runs appears in the next session, or after `/reload-plugins`.

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
