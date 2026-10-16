# Claude Code Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Claude Code's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Hooks reference](https://docs.anthropic.com/en/docs/claude-code/hooks)
> · [Hooks guide](https://docs.anthropic.com/en/docs/claude-code/hooks-guide)
> · [Extending Claude Code](https://docs.anthropic.com/en/docs/claude-code/features-overview)

Hooks are user-defined shell commands, HTTP endpoints, or LLM prompts that execute at specific points in the agent lifecycle. They provide deterministic control — actions always happen rather than relying on the model.

## Hook Types

| Type | Description |
|---|---|
| `command` | Shell script; communicates via stdin/stdout/exit codes |
| `http` | POSTs JSON to a URL endpoint; supports header interpolation with `$VAR_NAME` |
| `prompt` | Single-turn LLM evaluation returning `{ok: true/false, reason}` |
| `agent` | Spawns a subagent with tool access (Read, Grep, Glob) for up to 50 turns |

## Events

| Event | Trigger | Can block? | Matcher target |
|---|---|---|---|
| `SessionStart` | Session begins/resumes | No | `startup`, `resume`, `clear`, `compact` |
| `SessionEnd` | Session terminates (1.5s default timeout) | No | `clear`, `resume`, `logout`, etc. |
| `UserPromptSubmit` | User submits prompt, before processing | Yes (exit 2) | None |
| `PreToolUse` | Before tool call | Yes | Tool name regex (`Bash`, `Edit\|Write`, `mcp__.*`) |
| `PostToolUse` | After tool succeeds | No | Tool name regex |
| `PostToolUseFailure` | Tool fails | No | Tool name regex |
| `PermissionRequest` | Permission dialog appears | Yes | Tool name regex |
| `PermissionDenied` | Auto-mode classifier denial | No (`retry: true` available) | Tool name regex |
| `Stop` | Main agent finishes responding | Yes | None |
| `StopFailure` | Turn ends on API error | No (output ignored) | `rate_limit`, `authentication_failed`, etc. |
| `SubagentStart` | Subagent spawned | No | Agent type |
| `SubagentStop` | Subagent finishes | Yes | Agent type |
| `Notification` | System notification | No | `permission_prompt`, `idle_prompt`, etc. |
| `TaskCreated` | Task created | Yes (exit 2 rolls back) | None |
| `TaskCompleted` | Task completed | Yes (exit 2 rolls back) | None |
| `TeammateIdle` | Teammate about to go idle | Yes | None |
| `ConfigChange` | Config file changes during session | Yes (except `policy_settings`) | Config source |
| `CwdChanged` | Directory change | No | None |
| `FileChanged` | Watched file changes | No | Basename |
| `WorktreeCreate` | Git worktree created | Yes (non-zero fails) | None |
| `WorktreeRemove` | Git worktree removed | No | None |
| `PreCompact` | Before compaction | No | `manual`, `auto` |
| `PostCompact` | After compaction | No | `manual`, `auto` |
| `InstructionsLoaded` | CLAUDE.md loaded | No | Load reason |
| `Elicitation` | MCP server requests user input | Yes | MCP server name |
| `ElicitationResult` | MCP elicitation result | Yes | MCP server name |

## Configuration

Settings merge with precedence (highest first): **Managed → Command line → Local → Project → User**.

| File | Scope |
|---|---|
| Managed policy (MDM, registry, server, `/etc/claude-code/`) | Organization-wide |
| `.claude/settings.local.json` | Single project, gitignored |
| `.claude/settings.json` | Single project, committable |
| `~/.claude/settings.json` | All projects (user) |

`CLAUDE_CONFIG_DIR` moves the user level as a whole: when it is set, Claude Code
reads `$CLAUDE_CONFIG_DIR/settings.json`, `$CLAUDE_CONFIG_DIR/.claude.json` and
`$CLAUDE_CONFIG_DIR/skills/`, and ignores `~/.claude/settings.json`,
`~/.claude.json` and `~/.claude/skills/`. Symposium writes its global hooks,
user-level MCP servers and plugins there too. Verified against Claude Code
2.1.284 with the variable set: a hook and an MCP server left in the `~/` files
were ignored, while symposium's hooks under `$CLAUDE_CONFIG_DIR` fired
(`hook_started`/`hook_response` for `SessionStart` in
`claude -p --output-format stream-json --verbose`) and `claude mcp list` showed
its server.

### Configuration structure

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "./validate.sh",
            "if": "Bash(rm *)",
            "timeout": 60,
            "statusMessage": "Validating...",
            "async": false,
            "shell": "bash"
          }
        ]
      }
    ]
  }
}
```

- **`matcher`**: regex matched against event-specific values (tool name, session source, notification type).
- **`if`**: permission-rule syntax for additional filtering on tool events (e.g., `Bash(git *)`, `Edit(*.ts)`).

## Input Schema (stdin)

### Base fields (all events)

```json
{
  "session_id": "string",
  "transcript_path": "string",
  "cwd": "string",
  "permission_mode": "default|plan|auto|bypassPermissions|...",
  "hook_event_name": "string"
}
```

### PreToolUse additions

- `tool_name`: string
- `tool_input`: object with tool-specific fields (`command` for Bash, `file_path`/`content` for Write, etc.)
- `tool_use_id`: string

### PostToolUse additions

- `tool_name`, `tool_input`, `tool_use_id` (same as PreToolUse)
- `tool_response`: string (tool output)

### Stop additions

- `stop_hook_active`: boolean
- `last_assistant_message`: string

## Output Schema (stdout)

Output is capped at **10,000 characters**.

### Universal fields

| Field | Type | Description |
|---|---|---|
| `continue` | boolean | `false` stops Claude entirely |
| `stopReason` | string | Message for user when `continue` is false |
| `systemMessage` | string | Warning shown to user |
| `suppressOutput` | boolean | Omits stdout from debug log |

### PreToolUse decision output

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow|deny|ask|defer",
    "permissionDecisionReason": "string",
    "updatedInput": { "command": "safe-cmd" },
    "additionalContext": "string"
  }
}
```

Decision precedence across parallel hooks: **deny > defer > ask > allow**. The `allow` decision does **not** override deny rules from settings. `updatedInput` replaces the entire tool input; if multiple hooks return it, the last to finish wins (non-deterministic).

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed as JSON |
| `2` | Blocking error — action blocked, stderr fed to Claude |
| Other | Non-blocking warning, action proceeds |

## Execution Behavior

- All matching hooks run **in parallel**.
- Identical handlers deduplicated by command string or URL.
- Default timeouts: **600s** (command), **30s** (prompt), **60s** (agent), **1.5s** (SessionEnd, overridable via `CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS`).

## Environment Variables

| Variable | Description |
|---|---|
| `CLAUDE_PROJECT_DIR` | Absolute path to project root |
| `CLAUDE_ENV_FILE` | File for persisting env vars (SessionStart, CwdChanged, FileChanged only) |
| `CLAUDE_CODE_REMOTE` | `"true"` in remote web environments |

## Enterprise Controls

- **`allowManagedHooksOnly: true`** — blocks user/project/plugin hooks.
- **`allowedHttpHookUrls`** — restricts HTTP hook destinations.
- **`disableAllHooks: true`** — disables everything.
- PreToolUse deny blocks even in `bypassPermissions` mode.

## MCP Server Registration

In addition to hooks, symposium registers itself as an MCP server in the
agent's MCP configuration. This provides an alternative integration path
alongside the hook-based approach.

### Configuration structure

The entry goes under `mcpServers` - but **not** in the `settings.json` that
carries hooks. Claude Code does not read `mcpServers` from a settings file at
either scope; entries written there are silently ignored (`claude mcp list`
shows nothing, and a session sees no tools). MCP has its own two files:

```json
{
  "mcpServers": {
    "symposium": {
      "command": "/path/to/cargo-agents",
      "args": ["mcp"]
    }
  }
}
```

- **Project-level**: `<project>/.mcp.json`
- **User-level**: `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`)

Confirmed against the CLI: `claude mcp add -s project` writes the former,
`-s user` the latter.

A server in a project `.mcp.json` starts out **pending approval** - Claude asks
before trusting a server a repository supplies. Symposium deliberately does not
pre-approve its own entries (that would mean overriding a trust prompt on the
user's behalf); approve once via `/mcp`, or add the name to
`enabledMcpjsonServers` in your own settings if you want it standing.

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.

## Plugin Delivery

A plugin enabled with `use --global` reaches Claude Code as a
**skills-directory plugin**: symposium copies the compiled plugin directory
into the user skills directory, `~/.claude/skills/<plugin>/`
(`$CLAUDE_CONFIG_DIR/skills/<plugin>/` when that variable is set, in which case
Claude Code ignores `~/.claude/skills/`). Claude Code loads every folder there
that holds `.claude-plugin/plugin.json` as the plugin `<plugin>@skills-dir`, in
place, in every project. Its skills are invoked as `/<plugin>:<skill>`.

Nothing is written to `settings.json`, `known_marketplaces.json` or
`installed_plugins.json`. A user who runs `claude plugin disable
<plugin>@skills-dir` gets an `enabledPlugins` entry that symposium never
touches.

Verified against Claude Code 2.1.284 by asking the agent (`claude plugin list
--json`, `claude plugin details`, and the `system/init` event of `claude -p`):

- The plugin is listed in the first session after `cargo agents sync`, and
  `claude plugin list` shows it without a prior session.
- An edit at the same version is picked up by the next session: the copy is
  read in place, and nothing is cached under `~/.claude/plugins/cache/`.
- Removing the directory removes the plugin, with no residue and no error.
- The plugin takes its identity from its manifest, not its directory name, so
  a copy moved to a hashed directory name keeps its name.
- A plugin written during the `SessionStart` hook (symposium's auto-sync) loads
  in the next session, or after `/reload-plugins` in the current one.
- A plain `SKILL.md` folder of the user's own next to it is unaffected.

The alternative, registering the staging root as a directory marketplace
(`extraKnownMarketplaces` plus `enabledPlugins`), also loads in place, but
Claude Code then writes `~/.claude/plugins/known_marketplaces.json` and
`installed_plugins.json` entries of its own that outlive the registration, and
at project scope two repositories using the same marketplace name loaded each
other's content.

### Project scope

A `<project>/.claude/skills/<plugin>/` folder with the same layout loads as a
project-scoped `<plugin>@skills-dir`, only in a folder the user has trusted (an
untrusted folder lists it as `(suppressed)@skills-dir`; plain project skills
still load). A user-level plugin of the same name shadows the project one, and
`claude plugin list` reports it. Symposium does not write project-scoped
plugins yet.
