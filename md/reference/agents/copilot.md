# GitHub Copilot

Config name: `copilot`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | *(none)* |

Copilot has no global skills path.

## Hooks

Symposium creates a `symposium.json` file in the project hooks directory, and merges entries into the global config.

| Scope | File |
|-------|------|
| Project | `.github/hooks/symposium.json` |
| Global | `~/.copilot/config.json` |

Events registered: `preToolUse`, `postToolUse`, `userPromptSubmitted`, `sessionStart` (camelCase).

Output format: JSON. Uses `"bash"` key instead of `"command"` for platform-specific dispatch. Any non-zero exit code denies (not just exit 2).

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.vscode/mcp.json` | `<name>` (top-level) |
| Global | `~/.copilot/mcp-config.json` | `<name>` (top-level) |
