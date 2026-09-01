# Antigravity CLI

Config name: `antigravity`

Google's Antigravity CLI, invoked as `agy`. Antigravity ships three surfaces —
the CLI, the IDE and the web app — which share the same configuration roots, so
what symposium writes applies to all of them.

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.gemini/config/skills/<name>/SKILL.md` |

## Hooks

Symposium merges a single named entry, `symposium`, into Antigravity's
`hooks.json`. Other entries in the file are left alone, and only symposium's own
is removed when the agent is unconfigured.

| Scope | File |
|-------|------|
| Project | `.agents/hooks.json` |
| Global | `~/.gemini/config/hooks.json` |

Events registered: `PreToolUse`, `PostToolUse`, `PreInvocation`, `SessionStart`,
`Stop`.

Output format: JSON. Timeouts are in **seconds** (30 by default). Exit codes are
ignored — only what a hook writes to stdout affects the agent.

**Caveat:** `PreInvocation` stands in for symposium's `user-prompt-submit`
because Antigravity has no prompt event. It fires before *every* model call, so
symposium runs the prompt event only on the first invocation of a turn.

**Caveat:** in headless print mode (`agy -p`), Antigravity adopts no workspace
unless it is given `--add-dir <absolute path>` — it cannot read project files and
loads no project `.agents/` configuration. A relative path is ignored. Ordinary
interactive use is unaffected; this only matters for automation and CI.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.agents/mcp_config.json` | `mcpServers.<name>` |
| Global | `~/.gemini/config/mcp_config.json` | `mcpServers.<name>` |

Unlike Gemini CLI, MCP configuration lives in its own file rather than sharing
one with hooks. `agy mcp add` has no scope flag and writes the global file.
