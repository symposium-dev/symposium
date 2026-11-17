# Kiro Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Kiro's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [CLI hooks](https://kiro.dev/docs/cli/hooks/)
> · [Agent configuration reference](https://kiro.dev/docs/cli/custom-agents/configuration-reference)
> · [IDE hooks](https://kiro.dev/docs/hooks/)

Kiro is Amazon's AI coding agent available as an IDE (VS Code fork) and CLI. Both have hook systems but they differ in configuration format, trigger types, and capabilities.

## Kiro CLI Agent Definition

Each `.kiro/agents/*.json` file defines a complete agent. All fields are optional; omitting a field has specific defaults.

| File | Scope |
|---|---|
| `.kiro/agents/*.json` | Project |
| `~/.kiro/agents/*.json` | Global |

### Agent Definition Fields

| Field | Type | Default if omitted |
|---|---|---|
| `name` | string | Derived from filename |
| `description` | string | *(none)* |
| `prompt` | string or `file://` URI | No custom system context |
| `tools` | array of strings | **No tools available** |
| `allowedTools` | array of strings/globs | All tools require confirmation |
| `toolAliases` | object | *(none)* |
| `resources` | array of URIs/objects | *(none)* |
| `hooks` | object | *(none)* |
| `mcpServers` | object | *(none)* |
| `toolsSettings` | object | *(none)* |
| `includeMcpJson` | boolean | *(none)* |
| `model` | string | System default |
| `keyboardShortcut` | string | *(none)* |
| `welcomeMessage` | string | *(none)* |

**Critical:** Omitting `tools` means the agent has **zero tools**. Use `"tools": ["*"]` for all tools, `"@builtin"` for built-ins only, or list specific tools.

### Tools Field Values

- `"*"` — all available tools
- `"@builtin"` — all built-in tools
- `"read"`, `"write"`, `"shell"` — specific built-in tools
- `"@server_name"` — all tools from an MCP server
- `"@server_name/tool_name"` — specific MCP tool

### AllowedTools Field

Specifies tools that execute without user confirmation. Supports exact matches and glob patterns (`"@server/read_*"`, `"@git-*/status"`). Does **not** support `"*"` wildcard for all tools.

### Resources Field

- `"file://README.md"` — load file into context at startup
- `"skill://.kiro/skills/**/SKILL.md"` — skill metadata loaded at startup, full content on-demand

Custom agents do **not** auto-discover skills. They require explicit `skill://` URIs in `resources`.

## Kiro CLI Hooks

Configured inside agent configuration JSON files. Shell commands receive JSON on stdin and use exit codes for control flow.

### Events

| Event | Trigger | Matcher? | Can block? |
|---|---|---|---|
| `agentSpawn` | Session starts | No | No |
| `userPromptSubmit` | User submits prompt | No | No |
| `preToolUse` | Before tool execution | Yes | **Yes** (exit 2) |
| `postToolUse` | After tool execution | Yes | No |
| `stop` | Agent finishes | No | No |

### Input Schema (stdin)

All events include `hook_event_name` and `cwd`.

**userPromptSubmit** adds:
- `prompt`: string

**preToolUse** adds:
- `tool_name`: string
- `tool_input`: object (full tool arguments)

**postToolUse** adds:
- `tool_name`: string
- `tool_input`: object
- `tool_response`: string

MCP tools use `@server/tool` naming (e.g., `@postgres/query`).

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout captured as context |
| `2` | **Block** (preToolUse only); stderr sent to LLM as reason |
| Other | Warning; stderr shown but execution continues |

### Matcher Patterns

- Tool name strings: `execute_bash`, `fs_write`, `read`
- Aliases: `shell`, `write`
- MCP server globs: `@git`, `@git/status`
- Wildcards: `*`
- Built-in group: `@builtin`
- No matcher = applies to all tools

### Execution Behavior

- Hooks execute **in array order** within each trigger type.
- Default timeout: **30 seconds** (30,000ms), configurable via `timeout_ms`.
- `cache_ttl_seconds`: default 0 (no caching). `agentSpawn` hooks are never cached.

### Configuration Example

```json
{
  "hooks": {
    "preToolUse": [
      {
        "matcher": "execute_bash",
        "command": "./scripts/validate.sh"
      }
    ],
    "postToolUse": [
      {
        "matcher": "fs_write",
        "command": "cargo fmt --all"
      }
    ],
    "agentSpawn": [
      {
        "command": "git status"
      }
    ]
  }
}
```

Each entry is a flat object with `command` (required) and optional `matcher`. There is no nested `hooks` array or `type` field.

## Kiro IDE Hooks

Stored as individual `.kiro.hook` files in `.kiro/hooks/`. Created via the Kiro panel UI or command palette.

### Hook File Format

```
name: Format on save
description: Run formatter after file saves
when:
  type: fileEdit
  patterns: **/*.ts
then:
  type: shellCommand
  command: npx prettier --write {file}
```

### Trigger Types (10)

| Type | Trigger |
|---|---|
| `promptSubmit` | User submits a prompt |
| `agentStop` | Agent finishes responding |
| `preToolUse` | Before tool execution |
| `postToolUse` | After tool execution |
| `fileCreate` | File created |
| `fileEdit` | File saved |
| `fileDelete` | File deleted |
| `preTaskExecution` | Before spec task runs |
| `postTaskExecution` | After spec task runs |
| `userTriggered` | Manual invocation |

The IDE adds file-event and spec-task triggers not available in the CLI.

### Action Types (2)

| Type | Description |
|---|---|
| `askAgent` | Sends a natural language prompt to the agent (consumes credits) |
| `shellCommand` | Runs locally; exit 0 = stdout added to context, non-zero = blocks on preToolUse/promptSubmit |

### IDE Tool Matching Categories

`read`, `write`, `shell`, `web`, `spec`, `*`, `@mcp`, `@powers`, `@builtin`, plus regex patterns with `@` prefix.

### IDE Execution Behavior

- Default timeout: **60 seconds**.
- `USER_PROMPT` env var is available for `promptSubmit` shell commands.

## Environment Variables

No dedicated environment variables are documented for CLI hook execution. Context is passed via stdin JSON. The IDE provides `USER_PROMPT` for `promptSubmit` shell command hooks.

## Custom Instructions (Steering)

Kiro uses "steering files" instead of a single instructions file:

| Scope | Path |
|---|---|
| Workspace | `.kiro/steering/*.md` |
| Global | `~/.kiro/steering/*.md` |
| Standard | `AGENTS.md` at workspace root (always included) |

Steering files support YAML frontmatter with four inclusion modes: Always, FileMatch (glob pattern), Manual (referenced via `#name` in chat), and Auto (description-based matching). Kiro also auto-generates `product.md`, `tech.md`, and `structure.md`.

## Skills

| Scope | Path |
|---|---|
| Workspace | `.kiro/skills/<name>/SKILL.md` |
| Global | `~/.kiro/skills/<name>/SKILL.md` |

Workspace skills take precedence over global skills with the same name. The default agent auto-discovers skills from both locations. Custom agents require explicit `skill://` URIs in their `resources` field. Skills use `SKILL.md` with YAML frontmatter (`name`, `description`).

## MCP Server Configuration

| Scope | Path |
|---|---|
| Workspace | `.kiro/settings/mcp.json` |
| Global | `~/.kiro/settings/mcp.json` |
| Agent-level | `mcpServers` field in `.kiro/agents/*.json` |

Priority: Agent config > Workspace > Global. Format is JSON with `mcpServers` key, supporting `command`/`args`/`env` for stdio and `url`/`headers` for remote servers.

## MCP Server Registration

In addition to hooks, symposium registers itself as an MCP server in the
agent's MCP config file. This provides an alternative integration path
alongside the hook-based approach.

### Configuration structure

The MCP server entry is added under `mcpServers`:

```json
{
  "mcpServers": {
    "symposium": {
      "command": "/path/to/symposium",
      "args": ["mcp"]
    }
  }
}
```

- **Project-level**: `.kiro/settings/mcp.json`
- **User-level**: `~/.kiro/settings/mcp.json`

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.
