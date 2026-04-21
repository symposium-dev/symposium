# GitHub Copilot Hooks Reference

> **Disclaimer:** This document reflects our current understanding of GitHub Copilot's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [About hooks](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-hooks)
> · [Using hooks](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/use-hooks)
> · [Hooks configuration](https://docs.github.com/en/copilot/reference/hooks-configuration)

GitHub Copilot hooks are available for the Cloud Agent (coding agent), Copilot CLI (GA February 2026), and VS Code (8-event preview). The system is command-only and repository-native.

## Hook Types

Only `type: "command"` is supported.

## Events

### Cloud Agent and CLI (6 events, lowerCamelCase)

| Event | Trigger | Can block? |
|---|---|---|
| `sessionStart` | New or resumed session | No |
| `sessionEnd` | Session completes or terminates | No |
| `userPromptSubmitted` | User submits a prompt | No |
| `preToolUse` | Before tool call | **Yes** |
| `postToolUse` | After tool completes (success or failure) | No |
| `errorOccurred` | Error during agent execution | No |

### VS Code (8 events, PascalCase, preview)

`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PreCompact`, `SubagentStart`, `SubagentStop`, `Stop`.

Only **`preToolUse`/`PreToolUse`** can make access-control decisions. All other events are observational.

## Configuration

### Cloud Agent and CLI

Hooks defined in **`.github/hooks/*.json`**. For the Cloud Agent, files must be on the repository's **default branch**.

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "./scripts/security-check.sh",
        "powershell": "./scripts/security-check.ps1",
        "cwd": "scripts",
        "env": { "LOG_LEVEL": "INFO" },
        "timeoutSec": 15,
        "comment": "Documentation string, ignored at runtime"
      }
    ]
  }
}
```

| Field | Type | Description |
|---|---|---|
| `type` | string | Must be `"command"` |
| `bash` | string | Command for Linux/macOS |
| `powershell` | string | Command for Windows |
| `cwd` | string | Working directory relative to repo root |
| `env` | object | Environment variables |
| `timeoutSec` | number | Default **30 seconds** |
| `comment` | string | Documentation, ignored at runtime |

There is **no `matcher` field** — hooks fire on all invocations of their event type. Tool-level filtering must be done inside the script by inspecting `toolName` from stdin.

### VS Code

Also reads hooks from `.claude/settings.json`, `.claude/settings.local.json`, and `~/.claude/settings.json` (Claude Code format compatibility). Converts lowerCamelCase to PascalCase and maps `bash`→`osx`/`linux`, `powershell`→`windows`.

## Input Schema (stdin)

### preToolUse

```json
{
  "timestamp": 1704614600000,
  "cwd": "/path/to/project",
  "toolName": "bash",
  "toolArgs": "{\"command\":\"rm -rf dist\",\"description\":\"Clean build\"}"
}
```

**Note**: `toolArgs` is a **JSON string**, not an object. Scripts must parse it (e.g., with `jq`).

### sessionStart

- `source`: `"new"` | `"resume"`
- `initialPrompt`: string

### sessionEnd

- `reason`: string

## Output Schema (stdout)

### preToolUse output (Cloud Agent / CLI)

```json
{
  "permissionDecision": "deny",
  "permissionDecisionReason": "Destructive operations blocked"
}
```

| Value | Meaning |
|---|---|
| `"allow"` | Permit the tool call |
| `"deny"` | Block the tool call |
| `"ask"` | Prompt user for confirmation |

Exit code 0 = allow (if no JSON output), non-zero = deny.

### VS Code output (preview, extended fields)

| Field | Type | Description |
|---|---|---|
| `continue` | boolean | `false` stops agent |
| `stopReason` | string | Message when continue is false |
| `systemMessage` | string | Warning shown to user |
| `hookSpecificOutput.permissionDecision` | string | `allow`, `deny`, `ask` |
| `hookSpecificOutput.updatedInput` | object | Replace tool arguments |
| `hookSpecificOutput.additionalContext` | string | Extra context for agent |

## Execution Behavior

- Hooks run **synchronously and sequentially** (array order).
- If the first hook returns `deny`, subsequent hooks are **skipped**.
- Recommended execution time: **under 5 seconds**.
- Default timeout: **30 seconds**. On timeout, hook is terminated and agent continues.
- Scripts read JSON from stdin (`INPUT=$(cat)`) and write to stdout; debug output goes to stderr.

## Environment Variables

No built-in variables beyond those specified in the hook's `env` field. The `cwd` field controls the working directory.

## Additional Extension Surfaces (not hooks, but related)

### Custom instructions (soft/probabilistic)

| File | Scope |
|---|---|
| `.github/copilot-instructions.md` | Repository-wide instructions |
| `.github/instructions/**/*.instructions.md` | Path-specific instructions (with `applyTo` globs) |
| `AGENTS.md` | Agent-mode instructions |
| `~/.copilot/copilot-instructions.md` | User-level (personal) |
| Organization-level instructions | Admin-configured |

Priority: Personal (user) > Repository (workspace) > Organization.

### MCP server configuration

| Scope | Config path | Root key |
|---|---|---|
| VS Code (workspace) | `.vscode/mcp.json` | `servers` |
| VS Code (user) | Via "MCP: Open User Configuration" command | `servers` |
| CLI | `~/.copilot/mcp-config.json` | `mcpServers` |

Note: VS Code uses `"servers"` as root key while the CLI uses `"mcpServers"`. MCP tools only work in Copilot's Agent mode. Supported transports: `local`/`stdio`, `http`/`sse`.

### MCP Server Registration

Symposium registers MCP servers in the Copilot config as top-level keys
(matching the CLI's `mcpServers` format, not VS Code's `servers` format):

```json
{
  "symposium": {
    "command": "/path/to/cargo-agents",
    "args": ["mcp"]
  }
}
```

- **Project-level**: `.vscode/mcp.json`
- **User-level**: `~/.copilot/mcp-config.json`

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.

### Copilot SDK (programmatic hooks)

The `@github/copilot-sdk` (Node.js, Python, Go, .NET, Java) provides callback-style hooks for applications embedding the Copilot runtime:

- `onPreToolUse` — can return `modifiedArgs`
- `onPostToolUse` — can return `modifiedResult`
- `onSessionStart`, `onSessionEnd`, etc.

### Agent firewall (Cloud Agent)

Network-layer control with deny-by-default domain allowlist, configured at org/repo level. Not a hook — controls outbound network access.
