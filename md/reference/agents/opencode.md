# OpenCode

Config name: `opencode`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

## Hooks

Symposium hooks are supported via a TypeScript plugin that bridges OpenCode's native hook system to `cargo-agents hook opencode <event>`. The plugin is embedded in the `cargo-agents` binary and installed automatically during `cargo agents init`.

| Scope | Path |
|-------|------|
| Project | `.opencode/plugins/symposium.ts` |
| Global | `~/.config/opencode/plugins/symposium.ts` |

The plugin is regenerated on each `init` if the binary has been upgraded. User-managed files at the same path (without the `@generated` header) are left untouched.

| OpenCode hook | Symposium event | Capabilities |
|---|---|---|
| `tool.execute.before` | `PreToolUse` | Modify tool args via `updatedInput`. Cannot block execution. |
| `tool.execute.after` | `PostToolUse` | Inject `additionalContext` into tool output. |

OpenCode speaks the Symposium canonical wire format — no agent-specific serialization is needed.

**Limitations:** OpenCode cannot block tool execution. If a Symposium hook returns a deny, the plugin injects a warning into the post-tool output. `UserPromptSubmit` and `SessionStart` have no OpenCode equivalents that can return data.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `opencode.json` | `mcp.<name>` |
| Global | `~/.config/opencode/opencode.json` | `mcp.<name>` |
