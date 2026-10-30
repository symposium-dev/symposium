# Antigravity CLI

Config name: `antigravity`

Google's Antigravity CLI, invoked as `agy`. Antigravity ships three surfaces —
the CLI, the IDE and the web app — which share the same configuration roots, so
what symposium writes applies to all of them.

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |

## Plugins

A plugin enabled with `cargo agents use --global` is installed for you as a plugin directory:

| Scope | Path |
|-------|------|
| Global | `~/.gemini/config/plugins/<plugin>/` |

Antigravity loads it in every project, and its skills are invoked as `/<plugin>:<skill>`. Symposium writes no settings for it, so turning the plugin off in Antigravity stays in effect. A plugin installed while a session runs appears in the next session, or after `/skills reload`.

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
ignored — only what a hook writes to stdout affects the agent. Context reaches the
model at session start and on the first `PreInvocation` of a turn; after a tool
call it does not (`PostToolUse` output carries nothing back), and on `PreToolUse`
only a denial's reason is shown.

**Caveat:** `PreInvocation` stands in for symposium's `user-prompt-submit`
because Antigravity has no prompt event. It fires before *every* model call, so
symposium runs the prompt event only on the first invocation of a turn.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.agents/mcp_config.json` | `mcpServers.<name>` |
| Global | `~/.gemini/config/mcp_config.json` | `mcpServers.<name>` |

Unlike Gemini CLI, MCP configuration lives in its own file rather than sharing
one with hooks. `agy mcp add` has no scope flag and writes the global file.
