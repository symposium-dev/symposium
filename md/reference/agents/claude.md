# Claude Code

Config name: `claude`

## Skills

Skills reach Claude Code inside plugin directories (below). A skill that no plugin directory can carry is installed on its own:

| Scope | Path |
|-------|------|
| Project | `.claude/skills/<name>/SKILL.md` |

Claude Code does not support the vendor-neutral `.agents/skills/` path.

## Plugins

Every plugin that applies to your project is installed as a plugin directory, at the scope that enabled it:

| Scope | Path | Plugins |
|-------|------|---------|
| Project | `.claude/skills/<plugin>/` | registry plugins active in the project, workspace members, enabled dependencies, `cargo agents use` without `--global` |
| Global | `~/.claude/skills/<plugin>/` (`$CLAUDE_CONFIG_DIR/skills/<plugin>/` when set) | `cargo agents use --global` |

Claude Code loads each one as `<plugin>@skills-dir`, and its skills are invoked as `/<plugin>:<skill>`. A global plugin loads in every project; a project plugin loads only in that project. Symposium writes no settings for them; `claude plugin disable <plugin>@skills-dir` turns one off, and symposium leaves that choice alone. A plugin installed while a session runs appears in the next session, or after `/reload-plugins`.

Claude Code loads project plugins only:

- in a folder you trust. Until then they are skipped, while plain project skills still load. After you accept the trust prompt, `/reload-plugins` or a new session loads them.
- when it starts in the directory that holds `.claude/skills/`, the workspace root. Started in a subdirectory, it does not load them.

A global plugin with the same name as a project plugin takes its place.

## Hooks

Symposium merges hook entries into Claude Code's `settings.json`.

| Scope | File |
|-------|------|
| Project | `.claude/settings.json` |
| Global | `~/.claude/settings.json` |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart`, `Stop` (PascalCase).

Output format: JSON with `hookSpecificOutput` wrapper. Exit code 2 blocks tool use.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.mcp.json` | `mcpServers.<name>` |
| Global | `~/.claude.json` | `mcpServers.<name>` |

MCP servers do not go in `settings.json`, which holds only the hooks. The global file honors `CLAUDE_CONFIG_DIR`.
