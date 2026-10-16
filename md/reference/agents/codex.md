# Codex CLI

Config name: `codex`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |

## Plugins

A plugin enabled with `cargo agents use --global` is installed for you as `codex plugin add` installs one:

| What | Where |
|------|-------|
| Marketplace | `[marketplaces.symposium]` in `~/.codex/config.toml`, pointing at `~/.symposium/installed/` |
| Enablement | `[plugins."<plugin>@symposium"]` in the same file |
| Plugin files | `~/.codex/plugins/cache/symposium/<plugin>/<version>/` |

All three follow `$CODEX_HOME` when it is set. Codex lists the plugin in `/plugins` in every project, and its skills as `<plugin>:<skill>`. Turning it off in `/plugins` writes `enabled = false`, which symposium leaves alone; `codex plugin remove` is undone by the next sync, so remove it with `cargo agents use --remove --global <plugin>`. A plugin installed while a session runs appears in the next session.

## Hooks

Symposium merges hook entries into Codex's `hooks.json`.

| Scope | File |
|-------|------|
| Project | `.codex/hooks.json` |
| Global | `~/.codex/hooks.json` |

Events registered: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart` (PascalCase).

Output format: JSON. Exit code 2 blocks tool use.

**Caveat:** Codex runs a hook only after you trust it. Until then it skips the hook, warning at startup; open `/hooks` in Codex to review and trust symposium's entries. A changed entry needs trusting again. Project hooks also require the project itself to be trusted.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.codex/config.toml` | `[mcp_servers.<name>]` |

Codex reads no project-level MCP file, so a project-scoped registration goes to the global file.
