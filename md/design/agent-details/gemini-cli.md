# Gemini CLI Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Gemini CLI's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Hooks reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md)
> · [Extensions reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md)
> · [GitHub repo](https://github.com/google-gemini/gemini-cli)

Gemini CLI's hook system (v0.26.0, January 2026) mirrors Claude Code's JSON-over-stdin contract and exit-code semantics. It adds model-level and tool-selection interception events unique to Gemini.

## Hook Types

Only `type: "command"` is currently supported.

## Events

| Event | Trigger | Can block? | Category |
|---|---|---|---|
| `BeforeTool` | Before tool invocation | Yes | Tool |
| `AfterTool` | After tool execution | Yes (block result) | Tool |
| `BeforeAgent` | User submits prompt, before planning | Yes | Agent |
| `AfterAgent` | Agent loop ends (final response) | Yes (retry/halt) | Agent |
| `BeforeModel` | Before sending request to LLM | Yes (mock response) | Model |
| `AfterModel` | After receiving LLM response (per-chunk during streaming) | Yes (redact) | Model |
| `BeforeToolSelection` | Before LLM selects tools | Filter tools only | Model |
| `SessionStart` | Session begins | No (advisory) | Lifecycle |
| `SessionEnd` | Session ends | No (best-effort) | Lifecycle |
| `Notification` | System notification (e.g., `ToolPermission`) | No (advisory) | Lifecycle |
| `PreCompress` | Before context compression | No (async, cannot block) | Lifecycle |

### Model-level events (unique to Gemini)

- **`BeforeModel`**: can swap models, modify temperature, or return a synthetic response to skip the LLM call entirely.
- **`BeforeToolSelection`**: can filter the candidate tool list using `toolConfig.mode` (`AUTO`/`ANY`/`NONE`) and `allowedFunctionNames` whitelists. Multiple hooks use **union aggregation** across allowed function lists.
- **`AfterModel`**: can redact or modify the LLM response per-chunk during streaming.

## Configuration

Four-tier precedence: **Project → User → System → Extensions**.

| File | Scope |
|---|---|
| `.gemini/settings.json` | Project |
| `~/.gemini/settings.json` | User |
| `/etc/gemini-cli/settings.json` | System |
| Extensions | Plugin-provided |

### Configuration structure

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "write_file|replace",
        "sequential": false,
        "hooks": [
          {
            "name": "secret-scanner",
            "type": "command",
            "command": "$GEMINI_PROJECT_DIR/.gemini/hooks/block-secrets.sh",
            "timeout": 5000,
            "description": "Prevent committing secrets"
          }
        ]
      }
    ]
  }
}
```

- **`matcher`**: regex for tool events, exact string for lifecycle events.
- **`sequential`**: boolean (default false). When true, hooks run in order with output chaining.
- **`timeout`**: milliseconds (default **60,000**).

## Input Schema (stdin)

### Base fields (all events)

```json
{
  "session_id": "string",
  "transcript_path": "string",
  "cwd": "string",
  "hook_event_name": "string",
  "timestamp": "2026-03-03T10:30:00Z"
}
```

### BeforeTool additions

- `tool_name`: string
- `tool_input`: object (raw model arguments)
- `mcp_context`: object (optional)
- `original_request_name`: string (optional)

### AfterTool additions

- `tool_name`, `tool_input` (same as BeforeTool)
- `tool_response`: object containing `llmContent`, `returnDisplay`, and optional `error`

### BeforeModel additions

- `llm_request`: object with `model`, `messages`, `config`, `toolConfig`

### BeforeAgent additions

- `prompt`: string (the user's original prompt text)

### AfterAgent additions

- `stop_hook_active`: boolean (loop detection)

## Output Schema (stdout)

### Universal fields

| Field | Type | Description |
|---|---|---|
| `decision` | string | `"allow"` or `"deny"` (alias `"block"`) |
| `reason` | string | Feedback sent to agent when denied |
| `systemMessage` | string | Displayed to user |
| `continue` | boolean | `false` kills agent loop |
| `stopReason` | string | Message when continue is false |
| `suppressOutput` | boolean | Hide from logs/telemetry |

### Event-specific output via `hookSpecificOutput`

**BeforeTool**: `tool_input` — merges with and overrides model arguments.

**AfterTool**:
- `additionalContext`: string appended to tool result
- `tailToolCallRequest`: object triggering a follow-up tool call

**AfterAgent**: when denied, `reason` is sent as a new prompt for retry.

**BeforeAgent**: `additionalContext` — string appended to the prompt for that turn. `decision: "deny"` discards the user's message from history; `continue: false` preserves it.

**BeforeModel**:
- `llm_request`: overrides outgoing request (swap model, modify temperature, etc.)
- `llm_response`: provides synthetic response that skips the LLM call

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed as JSON |
| `2` | System block — stderr used as reason |
| Other | Warning (non-fatal), action proceeds |

## Execution Behavior

- Hooks run **in parallel by default**; set `sequential: true` for ordered execution with output chaining.
- Default timeout: **60,000ms**.

## Environment Variables

| Variable | Description |
|---|---|
| `GEMINI_PROJECT_DIR` | Absolute path to project root |
| `GEMINI_SESSION_ID` | Current session ID |
| `GEMINI_CWD` | Current working directory |
| `CLAUDE_PROJECT_DIR` | Compatibility alias for `GEMINI_PROJECT_DIR` |

Environment redaction for sensitive variables (KEY, TOKEN patterns) is available but disabled by default.

## Migration from Claude Code

```bash
gemini hooks migrate --from-claude
```

Converts `.claude` configurations to `.gemini` format. Tool name mappings:

| Claude Code | Gemini CLI |
|---|---|
| `Bash` | `run_shell_command` |
| `Edit` | `edit_file` |
| `Write` | `write_file` |
| `Read` | `read_file` |

## Custom Instructions

Gemini CLI reads `GEMINI.md` files at multiple levels:

| Scope | Path |
|---|---|
| Global | `~/.gemini/GEMINI.md` |
| Project | `GEMINI.md` in CWD and parent directories up to `.git` root |
| Just-in-time | `GEMINI.md` discovered when tools access a file/directory |

The filename is configurable via `context.fileName` in `settings.json` (e.g., `["AGENTS.md", "GEMINI.md"]`). Supports `@file.md` import syntax for including content from other files.

## Skills

| Scope | Path | Notes |
|---|---|---|
| Workspace | `.agents/skills/` or `.gemini/skills/` | `.agents/` takes precedence |
| User | `~/.agents/skills/` or `~/.gemini/skills/` | `.agents/` takes precedence |
| Extension | `~/.gemini/extensions/<name>/skills/` | Bundled with extensions |

Skills use `SKILL.md` with YAML frontmatter (`name`, `description`). Metadata is injected at session startup; full content loads on demand via `activate_skill`.

## MCP Server Configuration

Configured under `mcpServers` in `.gemini/settings.json` or `~/.gemini/settings.json`:

```json
{
  "mcpServers": {
    "serverName": {
      "command": "path/to/executable",
      "args": ["--arg1"],
      "env": { "API_KEY": "$MY_TOKEN" },
      "timeout": 30000
    }
  }
}
```

Transport is auto-selected by key: `command`+`args` (stdio), `url` (SSE), `httpUrl` (streamable HTTP).

## MCP Server Registration

In addition to hooks, symposium registers itself as an MCP server in the
agent's settings file. This provides an alternative integration path
alongside the hook-based approach.

### Configuration structure

The MCP server entry is added under `mcpServers` in the same settings
file used for hooks:

```json
{
  "mcpServers": {
    "cargo-agents": {
      "command": "/path/to/cargo-agents",
      "args": ["mcp"]
    }
  }
}
```

- **Project-level**: `.gemini/settings.json`
- **User-level**: `~/.gemini/settings.json`

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.
