# Command Code

Config name: `commandcode`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

Symposium merges hook entries into Command Code's `settings.json`.

| Scope | File |
|-------|------|
| Project | `.commandcode/settings.json` |
| Global | `~/.commandcode/settings.json` |

Events registered: `PreToolUse`, `PostToolUse`, `SessionStart` (PascalCase). Symposium's entries carry no `matcher`: Command Code reads the field as a regex, so `"*"` is not valid there, and an entry without one runs for every tool.

Output format: JSON. Exit code 2 blocks tool use.

**Limits:**

- Command Code has no `UserPromptSubmit` event, and Symposium does not register `Stop`, so plugin hooks for those events never fire there.
- `PreToolUse` can allow or deny a call and attach context, but has no `updatedInput` and no `ask`. A plugin's rewrite is not applied: the original call runs and a warning is logged.
- Hooks are skipped entirely while Command Code is in plan mode.
- Plugin hooks see Command Code's own tool names and argument fields, like every other agent's. A `matcher` meant to fire on Command Code names `shell_command`, `read_file`, `write_file` or `edit_file`; the shell tool's input carries `command`, and `read_file` takes `absolute_path`.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.commandcode/mcp.json` | `mcpServers.<name>` |

Command Code also reads a project `.mcp.json`, but Claude Code reads that file too, with a differently shaped entry for the same server name, so registering in both would have each agent's sync rewrite the other's entry. Symposium writes only the user-level file, and a project-scoped registration goes there. Entries carry an explicit `transport` of `stdio` or `http`; an SSE server is skipped, since Command Code does not offer it.
