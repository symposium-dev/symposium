# Command Code Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Command Code's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Hooks](https://commandcode.ai/docs/hooks)
> · [MCP](https://commandcode.ai/docs/mcp)
> · [Skills](https://commandcode.ai/docs/skills)

Command Code runs shell-command hooks configured under the `hooks` key of a `settings.json` file. The shape is Claude-like: event arrays of matcher groups, each holding handler entries.

## Configuration

| File | Scope |
|---|---|
| `~/.commandcode/settings.json` | User-global |
| `<repo>/.commandcode/settings.json` | Project-scoped |

Precedence is project over user. Within an event, hooks fire in the order they appear (project first, then user).

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "shell",
      "hooks": [{ "type": "command", "command": "./guard.sh", "timeout": 10 }]
    }]
  }
}
```

Only handler type is `"command"`. `timeout` is in **seconds**: default 30, maximum 600.

`matcher` is optional and is a **regex** tested against the tool's display name (`SHELL`, `READ`, `WRITE`, `EDIT`; the lowercase literals `shell`, `read`, `write`, `edit` are the documented spelling, and `write|edit` matches several). Omitting it matches every tool, so `"*"` is not valid. `Stop` and `SessionStart` carry no tool, so a `matcher` on them **prevents the hook from firing**. Symposium therefore registers no `matcher` at all.

## Events

| Event | Trigger | Can block? |
|---|---|---|
| `PreToolUse` | Before a tool runs | Yes (denies the tool) |
| `PostToolUse` | After the tool returns | No (advisory) |
| `Stop` | The assistant finishes a turn | Yes (forces a revision, capped at 3 retries) |
| `SessionStart` | A session begins (startup, resume, clear) | No (injects context only) |

There is no `UserPromptSubmit`. Symposium registers `PreToolUse`, `PostToolUse` and `SessionStart`; `Stop` is left unregistered, as for Codex, because it would spawn a hook on every turn to do nothing.

## Input Schema (stdin)

Common fields on every event: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `permission_mode` (`default`, `auto-accept`, `plan`, `bypass`, `dont-ask`, or empty).

Tool events add `tool_use_id`, `tool_name`, `tool_display_name` and `tool_input`. `PostToolUse` adds `tool_response` (a string). `SessionStart` adds `source` (`startup`, `resume` or `clear`).

| `tool_name` | `tool_display_name` | `tool_input` fields |
|---|---|---|
| `shell_command` | `SHELL` | `command`, `args?`, `directory?`, `timeout?` |
| `read_file` | `READ` | `absolute_path`, `offset?`, `limit?` |
| `write_file` | `WRITE` | `file_path`, `content` |
| `edit_file` | `EDIT` | `file_path`, `old_value`, `new_value`, `replacement_count?`, `replace_all?` |

MCP tools reach hooks under their own names. Symposium passes `tool_name` and `tool_input` to plugin hooks exactly as Command Code sends them.

## Output Schema (stdout)

Empty stdout on exit 0 means "no opinion, allow". Common fields: `continue` (`false` halts the session after the current tool batch), `stopReason`, `systemMessage`, `suppressOutput`.

`PreToolUse`:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked",
    "additionalContext": "Extra info for the model"
  }
}
```

`permissionDecision` is `"allow"` or `"deny"`. There is no `updatedInput` and no `ask`, so a hook can only allow, deny or annotate a call.

`PostToolUse` takes `hookSpecificOutput.additionalContext`, plus an advisory top-level `decision: "block"` with `reason` (the tool has already run). `SessionStart` takes `hookSpecificOutput.additionalContext`, which is injected into the session's first user turn. `Stop` uses top-level `decision: "block"` and `reason` only, with no `hookSpecificOutput`.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed as JSON |
| `2` | `PreToolUse`: blocked, stderr used as the reason. `PostToolUse`: advisory retry signal. `Stop`: retries the turn |
| Other | Non-blocking warning, the tool proceeds |

`SessionStart` never blocks: exit 2, `decision` and `continue: false` are ignored there.

## Execution Behavior

- `PreToolUse` hooks run sequentially and stop at the first deny; `PostToolUse`, `Stop` and `SessionStart` hooks run in parallel.
- Hooks are spawned through a system shell, with the input on stdin.
- Hooks are **skipped entirely in plan mode**.
- Environment variables set for hooks: `COMMANDCODE_PROJECT_DIR`, `COMMANDCODE_SESSION_ID`, `COMMANDCODE_HOOK_EVENT`, `COMMANDCODE_CWD`.

## Custom Instructions

| Scope | Path |
|---|---|
| Project | `AGENTS.md` |
| Global | ? |

## Skills

| Scope | Path |
|---|---|
| Project | `.agents/skills/<name>/SKILL.md` |
| User | `~/.agents/skills/<name>/SKILL.md` |

Command Code also reads `.commandcode/skills/`, which takes priority over `.agents/skills/` on a name clash. Symposium uses the vendor-neutral path.

## MCP Server Configuration

| Scope | File |
|---|---|
| Project | `<repo>/.mcp.json` |
| User | `~/.commandcode/mcp.json` |

Entries live under `mcpServers.<name>`, with `transport` (`stdio` or `http`), `command` and `args` for a local server, `url` and `headers` for a remote one, and `env`. Symposium writes only the user-level file, because Claude Code reads the project `.mcp.json` as well, with a differently shaped entry for the same name.
