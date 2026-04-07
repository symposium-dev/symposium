# Agents

`cargo-agents` supports multiple AI agents. Each agent has its own hook protocol, file layout, and configuration locations. This page documents the agent-specific details that `cargo-agents` needs to handle.

## Supported agents

| Config name | Agent |
|-------------|-------|
| `claude` | Claude Code |
| `copilot` | GitHub Copilot |
| `gemini` | Gemini CLI |

The agent name is stored in `[agent] name` in either the user or project config.

## Agent responsibilities

For each agent, `cargo-agents` needs to know how to:

1. **Register hooks** — write the hook configuration so the agent calls `cargo agents hook` on the right events.
2. **Install extensions** — place skill files (and eventually workflow/MCP definitions) where the agent expects them.

Where these files go depends on whether the agent is configured at the user level or the project level (see [`sync --agent`](./sync-agent-flow.md)).

## Extension locations

When installing skills, `cargo-agents` prefers vendor-neutral paths where possible:

| Scope | Path | Supported by |
|-------|------|-------------|
| Project skills | `.agents/skills/<skill-name>/SKILL.md` | Copilot, Gemini (preferred over `.gemini/skills/`) |
| Project skills | `.claude/skills/<skill-name>/SKILL.md` | Claude Code (does not support `.agents/skills/`) |

At the project level, Claude Code requires `.claude/skills/`, while Copilot and Gemini both support `.agents/skills/`. `cargo-agents` uses the vendor-neutral `.agents/skills/` path whenever the agent supports it.

At the global level, each agent has its own path:

| Agent | Global skills path |
|-------|-------------------|
| Claude Code | `~/.claude/skills/<skill-name>/SKILL.md` |
| Copilot | *(no global skills path)* |
| Gemini | `~/.gemini/skills/<skill-name>/SKILL.md` |

---

## Claude Code

[Hooks reference](https://docs.anthropic.com/en/docs/claude-code/hooks) · [Settings reference](https://docs.anthropic.com/en/docs/claude-code/settings) · [Skills reference](https://docs.anthropic.com/en/docs/claude-code/skills)

### Hook registration

Claude Code hooks live under the `"hooks"` key in settings JSON files. Each event maps to an array of matcher groups, each containing an array of hook commands.

| Scope | File |
|-------|------|
| Global | `~/.claude/settings.json` |
| Project (shared) | `.claude/settings.json` |
| Project (personal) | `.claude/settings.local.json` |

Example hook registration:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "cargo agents hook claude pre-tool-use"
          }
        ]
      }
    ]
  }
}
```

### Supported events

Claude Code supports many hook events. The ones relevant to `cargo-agents` are:

| Event | Description |
|-------|-------------|
| `PreToolUse` | Before a tool is invoked. Can allow, block, or modify the tool call. |
| `PostToolUse` | After a tool completes. Used to track skill activations. |
| `UserPromptSubmit` | When the user submits a prompt. Used for skill nudges. |

Other events include `SessionStart`, `Stop`, `Notification`, `SubagentStart`, and more.

### Hook payload/output

Claude Code wraps hook-specific fields in a nested `hookSpecificOutput` object:

```json
{
  "continue": true,
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "additionalContext": "...",
    "updatedInput": "..."
  }
}
```

---

## GitHub Copilot

[Hooks reference](https://docs.github.com/en/copilot/reference/hooks-configuration) · [Using hooks (CLI)](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks) · [Skills reference](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/create-skills)

### Hook registration

Copilot hooks are defined in JSON files with a `version` field. Hook entries use platform-specific command keys (`bash`, `powershell`) rather than a single `command` field.

| Scope | File |
|-------|------|
| Global | `~/.copilot/config.json` (under `hooks` key) |
| Project | `.github/hooks/*.json` |

Example hook registration:

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "cargo agents hook copilot pre-tool-use",
        "timeoutSec": 10
      }
    ]
  }
}
```

Note: Copilot uses camelCase event names (`preToolUse`), unlike Claude Code's PascalCase (`PreToolUse`).

### Supported events

| Event | Description |
|-------|-------------|
| `preToolUse` | Before a tool is invoked. Can allow, deny, or modify tool args. |
| `postToolUse` | After a tool completes. |
| `sessionStart` | New session begins. Supports `command` and `prompt` types. |
| `sessionEnd` | Session completes. |
| `userPromptSubmitted` | When the user submits a prompt. |
| `errorOccurred` | When an error occurs. |

### Hook payload/output

Copilot uses a flat output structure (no nested `hookSpecificOutput`). The input payload has `toolName` and `toolArgs` (where `toolArgs` is a JSON string that must be parsed separately):

```json
{
  "permissionDecision": "allow",
  "permissionDecisionReason": "...",
  "modifiedArgs": { ... },
  "additionalContext": "..."
}
```

Valid `permissionDecision` values: `"allow"`, `"deny"`, `"ask"`.

---

## Gemini CLI

[Hooks reference](https://geminicli.com/docs/hooks/reference/) · [Configuration reference](https://geminicli.com/docs/reference/configuration/) · [Skills reference](https://geminicli.com/docs/cli/skills/) · [Extensions reference](https://geminicli.com/docs/extensions/reference/)

### Hook registration

Gemini CLI hooks live under the `"hooks"` key in `settings.json`. Hook groups use regex matchers for tool events and exact-string matchers for lifecycle events.

| Scope | File |
|-------|------|
| Global | `~/.gemini/settings.json` |
| Project | `.gemini/settings.json` |

Example hook registration:

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": ".*",
        "hooks": [
          {
            "name": "cargo-agents",
            "type": "command",
            "command": "cargo agents hook gemini pre-tool-use",
            "timeout": 10000
          }
        ]
      }
    ]
  }
}
```

Note: Gemini uses `BeforeTool` (not `PreToolUse`), and timeouts are in milliseconds (default: 60000).

### Supported events

| Event | Type | Description |
|-------|------|-------------|
| `BeforeTool` | Tool | Before a tool is invoked. |
| `AfterTool` | Tool | After a tool completes. |
| `BeforeToolSelection` | Tool | Before the LLM selects tools. |
| `BeforeModel` | Model | Before LLM requests. |
| `AfterModel` | Model | After LLM responses. |
| `BeforeAgent` | Lifecycle | Before agent loop starts. |
| `AfterAgent` | Lifecycle | After agent loop completes. |
| `SessionStart` | Lifecycle | When a session starts. |
| `SessionEnd` | Lifecycle | When a session ends. |
| `PreCompress` | Lifecycle | Before history compression. |
| `Notification` | Lifecycle | On notification events. |

### Hook payload/output

Gemini uses a structure similar to Claude Code, with a nested `hookSpecificOutput`:

```json
{
  "decision": "allow",
  "reason": "...",
  "hookSpecificOutput": {
    "hookEventName": "BeforeTool",
    "additionalContext": "...",
    "tool_input": { ... }
  }
}
```

The input payload includes `tool_name`, `tool_input`, `mcp_context`, `session_id`, and `transcript_path`.

---

## Adding a new agent

To add support for a new agent:

1. Add a variant to the `HookAgent` enum in `hook_schema.rs`.
2. Create an agent module (e.g., `hook_schema/newagent.rs`) implementing the `Agent` trait and the event-specific payload/output types.
3. Implement the `AgentHookPayload` and `AgentHookOutput` traits to convert between the agent's wire format and the internal `HookPayload`/`HookOutput` types.
4. Document the agent's hook registration locations and extension file layout in this page.
