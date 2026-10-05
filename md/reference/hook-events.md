# Symposium hook events

This page documents the JSON schemas for symposium-format hooks — the input your hook receives on stdin and the output it should write to stdout. Symposium converts to and from each agent's native wire format, so you only need to handle these canonical types.

## Events

| Event | Description |
|-------|-------------|
| `PreToolUse` | Before the agent invokes a tool. Can inject context, modify the tool input, or block the call. |
| `PostToolUse` | After a tool completes. Can inject context. |
| `UserPromptSubmit` | When the user submits a prompt. Can inject context. |
| `SessionStart` | When an agent session begins. Can inject context. |
| `Stop` | When the agent's turn ends. |

## Agent support

Symposium registers one handler per event with each agent, under the agent's own name for that event. A "not delivered" cell means Symposium registers no handler for that event on that agent. The table gives each agent's native event name; the name in parentheses is the agent's name in a hook's `format` and in `cargo agents hook <agent>`.

| Agent | `PreToolUse` | `PostToolUse` | `UserPromptSubmit` | `SessionStart` | `Stop` |
|-------|--------------|---------------|--------------------|----------------|--------|
| Claude Code (`claude`) | `PreToolUse` | `PostToolUse` | `UserPromptSubmit` | `SessionStart` | `Stop` |
| Antigravity CLI (`antigravity`) | `PreToolUse` | `PostToolUse` | `PreInvocation` (first invocation of each turn only) | `SessionStart` | `Stop` |
| Codex CLI (`codex`) | `PreToolUse` | `PostToolUse` | `UserPromptSubmit` | `SessionStart` | not delivered |
| GitHub Copilot (`copilot`) | `preToolUse` | `postToolUse` | `userPromptSubmitted` | `sessionStart` | not delivered |
| Kiro (`kiro`) | `preToolUse` | `postToolUse` | `userPromptSubmit` | `agentSpawn` | not delivered |
| Goose | not delivered | not delivered | not delivered | not delivered | not delivered |
| OpenCode | not delivered | not delivered | not delivered | not delivered | not delivered |

Goose and OpenCode have no shell hooks, so no plugin hook ever runs there; they receive skills and MCP servers only. A hook for an event an agent does not deliver simply never fires on that agent. Agents also differ in what they do with a hook's output; for example, Antigravity shows a `PreToolUse` hook's output only when the hook blocks the call. Each agent's page under [Supported agents](./supported-agents.md) has the details.

## Input schemas

Your hook receives one of the following JSON objects on stdin, depending on which event it is registered for.

### `PreToolUse`

```json
{
  "PreToolUse": {
    "tool_name": "Bash",
    "tool_input": { "command": "cargo test" },
    "session_id": "abc-123",
    "cwd": "/home/user/project"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | string | Name of the tool being invoked. |
| `tool_input` | object | Arguments the agent is passing to the tool. |
| `session_id` | string or null | Agent session identifier, if available. |
| `cwd` | string or null | Working directory of the agent. |

### `PostToolUse`

```json
{
  "PostToolUse": {
    "tool_name": "Bash",
    "tool_input": { "command": "cargo test" },
    "tool_response": { "stdout": "test result: ok" },
    "session_id": "abc-123",
    "cwd": "/home/user/project"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | string | Name of the tool that was invoked. |
| `tool_input` | object | Arguments passed to the tool. |
| `tool_response` | object | The tool's response/output. |
| `session_id` | string or null | Agent session identifier, if available. |
| `cwd` | string or null | Working directory of the agent. |

### `UserPromptSubmit`

```json
{
  "UserPromptSubmit": {
    "prompt": "Fix the failing test in src/lib.rs",
    "session_id": "abc-123",
    "cwd": "/home/user/project"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `prompt` | string | The text the user submitted. |
| `session_id` | string or null | Agent session identifier, if available. |
| `cwd` | string or null | Working directory of the agent. |

### `SessionStart`

```json
{
  "SessionStart": {
    "session_id": "abc-123",
    "cwd": "/home/user/project"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string or null | Agent session identifier, if available. |
| `cwd` | string or null | Working directory of the agent. |

### `Stop`

```json
{
  "Stop": {
    "session_id": "abc-123",
    "cwd": "/home/user/project"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string or null | Agent session identifier, if available. |
| `cwd` | string or null | Working directory of the agent. |

## Output schemas

Your hook writes a JSON object to stdout. The object is wrapped in an enum tag matching the event, just like the input.

### `PreToolUse` output

```json
{
  "PreToolUse": {
    "additionalContext": "Remember to use --release for benchmarks",
    "updatedInput": { "command": "cargo test --release" }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `decision` | `"allow"` or `"deny"` | Whether to allow or block the tool call. Defaults to `"allow"` and may be omitted. With `"deny"`, put the reason in `additionalContext`; the SDK's `PreToolUseOutput::deny(reason)` writes both. |
| `additionalContext` | string or null | Text injected into the agent's context for this tool call. |
| `updatedInput` | object or null | Replacement tool input. If set, overrides the original `tool_input`. |

To block the call:

```json
{
  "PreToolUse": {
    "decision": "deny",
    "additionalContext": "Refusing to run a command that deletes the filesystem root"
  }
}
```

### `PostToolUse` output

```json
{
  "PostToolUse": {
    "additionalContext": "Note: 3 tests were skipped due to missing fixtures"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `additionalContext` | string or null | Text injected into the agent's context after the tool result. |

### `UserPromptSubmit` output

```json
{
  "UserPromptSubmit": {
    "additionalContext": "Relevant context: this project uses tokio 1.x"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `additionalContext` | string or null | Text injected into the agent's context for this prompt. |

### `SessionStart` output

```json
{
  "SessionStart": {
    "additionalContext": "symposium 0.5.0 is available (current: 0.4.2). Run `cargo agents self-update` to upgrade."
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `additionalContext` | string or null | Text injected into the agent's context at session start. |

### `Stop` output

```json
{
  "Stop": {
    "additionalContext": "Things look good!"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `additionalContext` | string or null | Text for the agent when the turn ends. Only Antigravity and Claude Code run `Stop` hooks, and neither treats this as plain context: Claude Code ignores it, while Antigravity takes it as a reason to keep going and starts another turn. |

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success. Stdout is parsed as JSON and merged into the hook result. |
| `2` | Block. The action is blocked and stderr is returned to the agent as the reason. |
| Other non-zero | Warning. The hook is considered to have succeeded for dispatch purposes; stdout is still parsed if possible. |

## Matcher

The `matcher` field on a hook entry is a regex matched against `tool_name` for `PreToolUse` and `PostToolUse` events. For `UserPromptSubmit`, `SessionStart`, and `Stop`, the matcher is ignored (all hooks fire). Use `"*"` to match all tools.

The regex is unanchored: it matches if it is found anywhere in the tool name, so `Bash` also matches `BashOutput`. Anchor it with `^...$` (`^Bash$`) to match one tool exactly.

`tool_name` is passed through exactly as the agent sends it, and `tool_input` keeps the agent's own argument names. Agents name the same tool differently: the shell tool is `Bash` in Claude Code and Codex, `bash` in Copilot, `execute_bash` in Kiro, and `run_command` in Antigravity, whose input carries `CommandLine` rather than `command`. A symposium-format hook meant to fire on several agents must match each name, for example `matcher = "^(Bash|bash|run_command|execute_bash)$"`, and read the input in each shape.

## Testing

To unit-test a symposium-format handler, pipe canonical JSON straight into it, the way Symposium does (a `script` handler runs under `sh`):

```bash
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"rm -rf /"},"session_id":null,"cwd":"/tmp"}}' \
  | sh scripts/check.sh
```

To test end to end, pipe the agent's own native JSON into [`cargo agents hook <agent> <event>`](./cargo-agents-hook.md), where `<agent>` is one of `antigravity`, `claude`, `codex`, `copilot`, or `kiro` and `<event>` is `pre-tool-use`, `post-tool-use`, `user-prompt-submit`, `session-start`, or `stop`. For Claude Code:

```bash
echo '{"session_id":"s","cwd":"/path/to/project","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test"}}' \
  | cargo agents hook claude pre-tool-use
```

This runs the full dispatch, including the automatic sync when `auto-sync` is on, for the workspace at `cwd`, so the plugin must be installed and active there. Symposium converts the payload to canonical JSON for your hook and prints the result in the agent's format (for Claude Code, a `hookSpecificOutput` object). There is no `symposium` agent name: the canonical format is only what your handler sees.
