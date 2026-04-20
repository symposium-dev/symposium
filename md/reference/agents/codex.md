# Codex CLI

Config name: `codex`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

Symposium merges hook entries into Codex's `hooks.json`.

| Scope | File |
|-------|------|
| Project | `.codex/hooks.json` |
| Global | `~/.codex/hooks.json` |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart` (PascalCase).

Output format: JSON. Exit code 2 blocks tool use.

**Caveat:** Codex hooks are experimental and disabled by default. To enable, add to `~/.codex/config.toml`:

```toml
[features]
codex_hooks = true
```

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.codex/config.toml` | `[mcp_servers.<name>]` |
| Global | `~/.codex/config.toml` | `[mcp_servers.<name>]` |
