# Codex CLI Hooks Reference

OpenAI's Codex CLI implements a shell-command hook system configured in `hooks.json`. It is **experimental** (disabled by default, not available on Windows) and first shipped in v0.114 (March 2026).

## Enabling

Add to `~/.codex/config.toml`:

```toml
[features]
codex_hooks = true
```

## Configuration

| File | Scope |
|---|---|
| `~/.codex/hooks.json` | User-global |
| `<repo>/.codex/hooks.json` | Project-scoped |

Both are **additive** — all matching hooks from all files run. Project hooks follow the untrusted-project trust model.

### Configuration structure

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "python3 ~/.codex/hooks/check_bash.py",
        "statusMessage": "Checking command safety",
        "timeout": 30
      }]
    }]
  }
}
```

Only handler type is `"command"`. `matcher` is a regex string; omit or use `""` / `"*"` to match everything. Default timeout: **600 seconds**, configurable via `timeout` or `timeoutSec`.

## Events

| Event | Trigger | Matcher filters on | Can block? |
|---|---|---|---|
| `SessionStart` | Session starts or resumes | `source` (`"startup"` or `"resume"`) | Yes (`continue: false`) |
| `PreToolUse` | Before tool execution | `tool_name` (currently only `"Bash"`) | Yes |
| `PostToolUse` | After tool execution | `tool_name` (currently only `"Bash"`) | Yes (`continue: false`) |
| `UserPromptSubmit` | User submits a prompt | N/A | Yes (`continue: false`) |
| `Stop` | Agent turn completes | N/A | Yes (deny → continuation prompt) |

## Input Schema (stdin)

### Base fields (all events)

```json
{
  "session_id": "string",
  "transcript_path": "string|null",
  "cwd": "string",
  "hook_event_name": "string",
  "model": "string"
}
```

Turn-scoped events (PreToolUse, PostToolUse, UserPromptSubmit, Stop) add `turn_id`.

### PreToolUse additions

- `tool_name`: string
- `tool_use_id`: string
- `tool_input`: object with `command` field

### PostToolUse additions

- `tool_name`, `tool_use_id`, `tool_input` (same as PreToolUse)
- `tool_response`: string

### UserPromptSubmit additions

- `prompt`: string

### Stop additions

- `stop_hook_active`: boolean
- `last_assistant_message`: string

## Output Schema (stdout)

### Deny/block (two equivalent methods)

Method 1 — JSON output:
```json
{ "decision": "block", "reason": "Destructive command blocked" }
```

or:

```json
{
  "hookSpecificOutput": {
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked"
  }
}
```

Method 2 — **Exit code 2** with reason on stderr.

### Inject context

```json
{
  "hookSpecificOutput": {
    "additionalContext": "Extra info for the agent"
  }
}
```

Plain text on stdout also works for SessionStart and UserPromptSubmit (ignored for PreToolUse, PostToolUse, Stop).

### Stop session

```json
{ "continue": false, "stopReason": "Session terminated by hook" }
```

Supported on SessionStart, UserPromptSubmit, PostToolUse, Stop.

### System message (UI warning)

```json
{ "systemMessage": "Warning text shown to user" }
```

### Stop event special behavior

For the Stop event, `{ "decision": "block", "reason": "Run tests again" }` tells Codex to create a **continuation prompt** — it does not reject the turn.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed. No output = continue normally. |
| `2` | Block/deny; stderr used as reason |
| Other | Non-blocking warning |

## Execution Behavior

- Multiple matching hooks run **concurrently** — no ordering guarantees.
- Commands run with session `cwd` as working directory.
- Shell expansion works.

## Parsed but Not Yet Implemented

These fields are accepted but **fail open** (no effect): `suppressOutput`, `updatedInput`, `updatedMCPToolOutput`, `permissionDecision: "allow"`, `permissionDecision: "ask"`.

## Current Limitations

- Only **Bash** tool events fire PreToolUse/PostToolUse — no file-write or MCP tool hooks.
- PreToolUse can only deny, **not modify** tool input.
- No async hook mode.
- Stop event requires JSON output (plain text is invalid).

## Environment Variables

No dedicated environment variables are set during hook execution (unlike Claude Code's `CLAUDE_PROJECT_DIR`). All context is passed via stdin JSON. The `cwd` field serves as the project directory equivalent. `CODEX_HOME` (defaults to `~/.codex`) controls where Codex stores config and state.

## Custom Instructions

| Scope | Path |
|---|---|
| Global | `~/.codex/AGENTS.md` (or `AGENTS.override.md`) |
| Project | `AGENTS.md` (or `AGENTS.override.md`) at each directory level from git root to CWD |

The `project_doc_fallback_filenames` config option in `~/.codex/config.toml` allows alternative filenames. Max combined size: 32 KiB (`project_doc_max_bytes`).

## Skills

| Scope | Path |
|---|---|
| Repository | `.agents/skills/<name>/SKILL.md` (each dir from CWD up to repo root) |
| User | `~/.agents/skills/<name>/SKILL.md` |
| Admin | `/etc/codex/skills/<name>/SKILL.md` |
| System | Bundled with Codex |

Skills use `SKILL.md` with YAML frontmatter (`name`, `description`) and may include `scripts/`, `references/`, `assets/`, and `agents/openai.yaml`.

## MCP Server Configuration

Configured in `~/.codex/config.toml` or `.codex/config.toml` under `[mcp_servers.<name>]`:

```toml
[mcp_servers.my-server]
command = "path/to/executable"
args = ["--arg1"]
env = { API_KEY = "value" }
startup_timeout_sec = 10
tool_timeout_sec = 60
```

Supports stdio (`command`/`args`) and streamable HTTP (`url`/`bearer_token_env_var`). CLI management: `codex mcp add <name> ...`.

## Other Extensibility

- `notify` in config.toml (fire-and-forget on agent-turn-complete)
- Execpolicy command-level rules
- Subagents
- Slash commands
