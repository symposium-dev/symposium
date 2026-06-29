# Symposium hook events

This page documents the JSON schemas for symposium-format hooks — the input your hook receives on stdin and the output it should write to stdout. Symposium converts to and from each agent's native wire format, so you only need to handle these canonical types.

## Events

| Event | Description |
|-------|-------------|
| `PreToolUse` | Before the agent invokes a tool. Can inject context or modify the tool input. |
| `PostToolUse` | After a tool completes. Can inject context. |
| `UserPromptSubmit` | When the user submits a prompt. Can inject context. |
| `SessionStart` | When an agent session begins. Can inject context. |

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
| `decision` | `"allow"` or `"deny"` | Whether to allow or block the tool call. Defaults to `"allow"` and may be omitted. |
| `additionalContext` | string or null | Text injected into the agent's context for this tool call. |
| `updatedInput` | object or null | Replacement tool input. If set, overrides the original `tool_input`. |

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

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success. Stdout is parsed as JSON and merged into the hook result. |
| `2` | Block. The action is blocked and stderr is returned to the agent as the reason. |
| Other non-zero | Warning. The hook is considered to have succeeded for dispatch purposes; stdout is still parsed if possible. |

## Matcher

The `matcher` field on a hook entry is a regex matched against `tool_name` for `PreToolUse` and `PostToolUse` events. For `UserPromptSubmit` and `SessionStart`, the matcher is ignored (all hooks fire). Use `"*"` to match all tools.

## Testing

You can test a symposium-format hook directly from the command line:

```bash
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"rm -rf /"},"session_id":null,"cwd":"/tmp"}}' \
  | ./scripts/check.sh
```

Or via the `cargo agents hook` CLI with the symposium format:

```bash
echo '{"PreToolUse":{"tool_name":"Bash","tool_input":{"command":"cargo test"},"session_id":null,"cwd":"/tmp"}}' \
  | cargo agents hook symposium pre-tool-use
```
