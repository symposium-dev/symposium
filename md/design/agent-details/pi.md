# Pi integration

Primary sources: [Pi extensions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md),
[skills](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md),
and [MCP](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/mcp.md).

## Extension registration

`src/agents/pi.rs` installs the embedded `src/agents/pi/extension.ts` into
`.pi/extensions/symposium.ts` at project scope or
`~/.pi/agent/extensions/symposium.ts` at user scope. The user path honors
`PI_CODING_AGENT_DIR`. Registration compares content before writing. The generated
header identifies files that Symposium may update or remove. An existing file
without that header is not overwritten or removed. When hook scope changes,
`sync` calls `Agent::register_scoped_hooks`, which removes the generated extension
at the other scope for that workspace.
Run `/reload` after a scope change to replace the active handlers.

The extension starts no work in its factory. Every handler checks
`ctx.isProjectTrusted()` before dispatch or project resource discovery, including
when the extension is installed globally. Event handlers run `cargo-agents
hook pi <event>` with JSON on stdin, using `execFile` without a shell. Prompts and
tool inputs are not shell arguments. Each process has a 10-minute timeout and a
1 MiB output limit. All events use this timeout because tools install lazily when
their hook first fires; session-start refresh does not install missing tools.
Active-turn cancellation stops its hook process.

## Event mapping and wire format

| Pi event | Symposium event |
|----------|-----------------|
| `session_start` | `SessionStart` |
| `input` | `UserPromptSubmit` |
| `tool_call` | `PreToolUse` |
| `tool_result` | `PostToolUse` |
| `agent_end` | `Stop` |

`src/hook_schema/pi.rs` uses the SDK's event-specific structs as the Pi bridge
wire format. The command-line event name supplies the tag. Each input carries
`cwd` and `session_id`; tool events also carry `tool_name` and `tool_input`.
Post-tool input includes Pi's content, details, structured content, and error
state in `tool_response`.

Output uses the SDK's flat fields: `decision`, `updatedInput`, and
`additionalContext`. Denial becomes Pi's `{ block: true, reason }`. Input updates
replace the existing input object's keys. Only an explicit `decision = "deny"`
blocks a tool. Process failures, missing binaries, timeouts, malformed output,
and non-object input updates let the tool run. Only the first hook error in each
session produces a warning. Cancellation produces no warning and does not count
as that first error. Warning state resets on `session_start`, not on turn end.
Warnings use Pi's UI when available, or stderr otherwise; they never use protocol
stdout.

Pre-tool context is stored by `toolCallId` and appended to that tool's result,
followed by any post-tool context. This lets the model read it on its next request
without waiting for another user prompt. The result keeps its structured content
and error state. A post-hook failure does not discard pre-tool context. Pending
context is removed after the result and cleared on turn end, session start, or
session shutdown.

Session-start, prompt, and Stop context use hidden custom messages for the next
user prompt, without requesting another turn.

Symposium's normal hook pipeline handles auto-sync and plugin format conversion.
Native `format = "pi"` plugins receive flat Pi bridge JSON; portable
`format = "symposium"` plugins receive tagged SDK JSON.

## Generated skill discovery

Issue [#248](https://github.com/symposium-dev/symposium/issues/248) identifies a
Pi-specific limit: Pi's directory scanner honors `.gitignore`, including the `*`
rule Symposium writes in generated skill directories.

Keep the shared `.agents/skills/` path and its ignore rules. Pi fires
`resources_discover` after `session_start`, so automatic sync installs skills
before the extension returns their explicit `SKILL.md` paths. Explicit files
bypass the directory scan. The extension checks the current directory and its
ancestors up to the repository root, plus `~/.agents/skills/`. Without a repository
root, it checks ancestors up to the filesystem root. It returns only
skills with the `.symposium` marker. User skills still follow Pi's normal rules.

A dependency change can install more skills during an active session. Pi needs
`/reload` to discover new resources and MCP entries. Removing a skill removes it
from the next resource-discovery result.

## MCP registration

Pi 1.0 reads `mcpServers` from `.pi/mcp.json` and
`~/.pi/agent/mcp.json`. Entries use `command`, `args`, and `env` for stdio, or
`url` and `headers` for streamable HTTP. `register_pi_mcp_servers` updates the
transport fields while preserving Pi-specific user options. SSE is unsupported
and is skipped with a warning.

## Tests

```bash
cargo test --test pi --test hook_context
cd src/agents/pi
npm ci --ignore-scripts
npm run check
npm test
```

Rust tests cover init, scope selection, automatic sync, cleanup, wire conversion,
and MCP updates. Node tests cover event handling, subprocess errors, input
changes, parallel calls, pre-tool context timing, warning-only hook failures,
skill discovery using Pi's real skill loader, and MCP file discovery using
`pi mcp list` with local test servers.
