# Pi

Config name: `pi`

Symposium supports Pi 1.0 or later, with Node.js 22.19 or later.

```bash
cargo agents init --add-agent pi
```

Run `cargo agents sync` and Pi's `/reload` after changing hook scope.

Start Pi in a Rust workspace. The Symposium extension syncs skills on session
start. It also checks for dependency changes when Pi submits a prompt or runs a
tool. Run `/reload` after adding skills or MCP servers during an active session.

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.agents/skills/<name>/SKILL.md` |

Pi's directory scanner respects `.gitignore`. Symposium's generated skills have
an ignore rule of `*`. The extension supplies explicit `SKILL.md` paths through
Pi's `resources_discover` event, so those skills load without changing the ignore
rules. User-managed skills keep Pi's normal discovery rules.

## Hooks

Pi uses TypeScript extensions, not shell-command hook settings. Symposium
installs one extension:

| Scope | File |
|-------|------|
| Project | `.pi/extensions/symposium.ts` |
| Global | `~/.pi/agent/extensions/symposium.ts` |

`PI_CODING_AGENT_DIR` changes the global agent directory. Project extensions load
only after Pi grants project trust. The bridge also skips sync and hooks in
untrusted projects when the extension is installed globally. If extensions are
disabled, use `cargo agents sync` manually; the generated skills still need the
extension for discovery.

| Pi event | Symposium event |
|----------|-----------------|
| `session_start` | `session-start` |
| `input` | `user-prompt-submit` |
| `tool_call` | `pre-tool-use` |
| `tool_result` | `post-tool-use` |
| `agent_end` | `stop` |

The bridge supports tool denial, input changes, and additional context. Only an
explicit `decision = "deny"` blocks a tool. Other hook errors let the tool run,
including a missing `cargo-agents` binary or a timeout. The bridge warns only once
per session. User cancellation produces no warning.

Each child process has a 10-minute timeout so first-time tool installation has
time to finish, including when installation starts during a tool event.

Pre-tool context is added to the matching tool result, so the model sees it
without another user prompt. Stop context does not start another agent turn.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.pi/mcp.json` | `mcpServers.<name>` |
| Global | `~/.pi/agent/mcp.json` | `mcpServers.<name>` |

Pi supports stdio and streamable HTTP. Symposium skips SSE servers with a
warning. It preserves user-set enabled state, exposure, and other Pi options.
The built-in MCP extension must be enabled; third-party MCP replacements can
use a different configuration format.
