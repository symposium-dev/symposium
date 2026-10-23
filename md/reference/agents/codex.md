# Codex CLI

Config name: `codex`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |

## Hooks

Symposium merges hook entries into Codex's `hooks.json`.

| Scope | File |
|-------|------|
| Project | `.codex/hooks.json` |
| Global | `~/.codex/hooks.json` (`$CODEX_HOME/hooks.json` when set) |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart` (PascalCase).

Output format: JSON. Exit code 2 blocks tool use.

**Caveat:** Codex runs a hook only after you trust it. Until then it skips the hook, warning at startup; open `/hooks` in Codex to review and trust symposium's entries. A changed entry needs trusting again. Project hooks also require the project itself to be trusted.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.codex/config.toml` (`$CODEX_HOME/config.toml` when set) | `[mcp_servers.<name>]` |

Codex reads no project-level MCP file, so a project-scoped registration goes to the global file.
