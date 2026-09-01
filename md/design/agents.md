# Agents

`cargo agents` supports multiple AI agents. Each agent has its own hook protocol, file layout, and configuration locations. This page documents the agent-specific details that `cargo agents` needs to handle.

## Supported agents

| Config name | Agent |
|-------------|-------|
| `antigravity` | Antigravity CLI |
| `claude` | Claude Code |
| `copilot` | GitHub Copilot |
| `codex` | Codex CLI |
| `kiro` | Kiro |
| `opencode` | OpenCode |
| `goose` | Goose |

The agent name is stored in `[agent] name` in either the user or project config.

## Agent responsibilities

For each agent, `cargo agents` needs to know how to:

1. **Register hooks** — write the hook configuration so the agent calls `cargo-agents hook` on the right events.
2. **Install extensions** — place skill files (and eventually workflow/MCP definitions) where the agent expects them.

Where these files go depends on whether the agent is configured at the user level or the project level (see [`sync --agent`](./sync-agent-flow.md)).

## Extension locations

When installing skills, `cargo agents` prefers vendor-neutral paths where possible:

| Scope | Path | Supported by |
|-------|------|-------------|
| Project skills | `.agents/skills/<skill-name>/SKILL.md` | Antigravity, Copilot, Codex, OpenCode, Goose |
| Project skills | `.claude/skills/<skill-name>/SKILL.md` | Claude Code (does not support `.agents/skills/`) |
| Project skills | `.kiro/skills/<skill-name>/SKILL.md` | Kiro (uses its own path) |

At the project level, Claude Code requires `.claude/skills/`, Kiro requires `.kiro/skills/`, while Antigravity, Copilot, Codex, OpenCode, and Goose all support `.agents/skills/`. `cargo agents` uses the vendor-neutral `.agents/skills/` path whenever the agent supports it.

At the global level, each agent has its own path:

| Agent | Global skills path |
|-------|-------------------|
| Antigravity CLI | `~/.gemini/config/skills/<skill-name>/SKILL.md` |
| Claude Code | `~/.claude/skills/<skill-name>/SKILL.md` |
| Copilot | *(no global skills path)* |
| Codex | `~/.agents/skills/<skill-name>/SKILL.md` |
| Kiro | `~/.kiro/skills/<skill-name>/SKILL.md` |
| OpenCode | `~/.agents/skills/<skill-name>/SKILL.md` |
| Goose | `~/.agents/skills/<skill-name>/SKILL.md` |

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
            "command": "cargo-agents hook claude pre-tool-use"
          }
        ]
      }
    ]
  }
}
```

### Supported events

Claude Code supports many hook events. The ones relevant to Symposium are:

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
        "bash": "cargo-agents hook copilot pre-tool-use",
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

## Antigravity CLI

[Hooks reference](./agent-details/antigravity-cli.md)

### Hook registration

Antigravity hooks live in a dedicated `hooks.json` under the customization root —
`.agents/` in a workspace, `~/.gemini/config/` globally. Entries are keyed by a
**hook name**, so symposium owns a single `symposium` key and leaves anything
else in the file untouched.

| Scope | Path |
|-------|------|
| Global | `~/.gemini/config/hooks.json` |
| Project | `.agents/hooks.json` |

```json
{
  "symposium": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "cargo-agents hook antigravity pre-tool-use",
            "timeout": 30
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "type": "command",
        "command": "cargo-agents hook antigravity session-start",
        "timeout": 30
      }
    ]
  }
}
```

Note the two shapes: tool events (`PreToolUse`, `PostToolUse`) wrap their
handlers in a `matcher` group, while lifecycle events (`PreInvocation`,
`SessionStart`, `Stop`) take a flat list. Timeouts are in seconds, not
milliseconds. An unrecognised shape or an unknown event name is accepted
silently and simply never fires.

`SessionStart` is missing from Antigravity's published documentation but present
in its hook proto, and fires once per session. There is no prompt event, so
`PreInvocation` stands in for `user-prompt-submit`; because it fires before every
model call, dispatch runs the prompt event only when `invocationNum == 0`.

### Hook output

Exit codes are ignored entirely — only stdout matters. On `PreToolUse`, an object
without a valid `decision` (including `{}`) is treated as a **denial**, so
symposium always emits `{"decision": "allow"}` unless a plugin actually denied.
Context is returned as an injected step rather than an `additionalContext` field:

```json
{ "injectSteps": [{ "ephemeralMessage": "..." }] }
```

---

## Kiro

[Hooks reference](./agent-details/kiro.md)

### Hook registration

Kiro hooks live in agent JSON files under `.kiro/agents/`. Symposium creates a `symposium.json` agent file with its hooks. Kiro uses camelCase event names.

| Scope | File |
|-------|------|
| Global | `~/.kiro/agents/symposium.json` |
| Project | `.kiro/agents/symposium.json` |

Example hook registration:

```json
{
  "hooks": {
    "preToolUse": [
      {
        "matcher": "*",
        "command": "cargo-agents hook kiro pre-tool-use"
      }
    ],
    "agentSpawn": [
      {
        "command": "cargo-agents hook kiro session-start"
      }
    ]
  }
}
```

Kiro uses a flat entry format: each entry has `command` directly (and optional `matcher`), with no nested `hooks` array or `type` field.

### Supported events

| Event | Description |
|-------|-------------|
| `preToolUse` | Before a tool is invoked. Can block (exit code 2). |
| `postToolUse` | After a tool completes. |
| `userPromptSubmit` | When the user submits a prompt. |
| `agentSpawn` | Session starts (maps to `session-start` internally). |
| `stop` | Agent finishes. |

### Hook payload/output

Kiro uses exit-code-based control flow:
- Exit 0: stdout captured as additional context
- Exit 2: block (`preToolUse` only), stderr as reason
- Other: warning, stderr shown

Input includes `hook_event_name`, `cwd`, `tool_name`, and `tool_input` on stdin as JSON.

### Unregistration

Unregistration deletes the `symposium.json` file.

---

## Codex CLI

[Hooks reference](./agent-details/codex-cli.md)

### Hook registration

Codex CLI hooks live in `hooks.json` files. The structure is similar to Claude Code — nested matcher groups with hook command arrays. Codex uses PascalCase event names and `timeout` in seconds.

| Scope | File |
|-------|------|
| Global | `~/.codex/hooks.json` |
| Project | `.codex/hooks.json` |

Example hook registration:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "",
        "hooks": [{
          "type": "command",
          "command": "cargo-agents hook codex pre-tool-use",
          "timeout": 10
        }]
      }
    ]
  }
}
```

Note: An empty `matcher` string matches everything in Codex (equivalent to `"*"` in other agents).

### Supported events

| Event | Description |
|-------|-------------|
| `PreToolUse` | Before a tool is invoked. Can block. |
| `PostToolUse` | After a tool completes. Can stop session (`continue: false`). |
| `UserPromptSubmit` | When the user submits a prompt. |
| `SessionStart` | Session starts or resumes. |
| `Stop` | Agent turn completes. |

### Hook payload/output

Codex uses a protocol similar to Claude Code, with two methods to block:
1. JSON output: `{ "decision": "block", "reason": "..." }`
2. Exit code 2 with reason on stderr

Also supports `hookSpecificOutput` with `additionalContext`, and `{ "continue": false }` to stop the session.

Input includes `session_id`, `cwd`, `hook_event_name`, `model`, `turn_id`, `tool_name`, `tool_use_id`, and `tool_input`.

---

## OpenCode

[Hooks reference](./agent-details/opencode.md)

### Hook registration

**OpenCode does not support shell-command hooks.** Its extensibility is based on TypeScript/JavaScript plugins. Symposium cannot register hooks for OpenCode.

### Supported events

OpenCode's plugin system supports these events, but Symposium does not currently bridge them:

| OpenCode event | Symposium event | Description |
|---|---|---|
| `tool.execute.before` | pre-tool-use | Before a built-in tool runs. Can block by throwing Error, or mutate `output.args`. |
| `tool.execute.after` | post-tool-use | After a built-in tool completes. |
| `message.updated` | user-prompt-submit | Filtered to `role === "user"` messages. |
| `session.created` | session-start | When a new session begins. |

---

## Goose

[Hooks reference](./agent-details/goose.md)

### Hook registration

**Goose does not implement lifecycle hooks.** It uses MCP extensions for extensibility. `symposium` cannot register hooks for Goose.

Goose is supported as a **skills-only** agent — `cargo agents sync` will install skill files in the vendor-neutral `.agents/skills/` path.

---

## Cross-agent event mapping

The following table maps symposium's internal event names to each agent's wire-format event name. `—` means the agent does not support shell-command hooks.

| Symposium event | Antigravity | Claude | Copilot | Codex | Kiro | OpenCode | Goose |
|---|---|---|---|---|---|---|---|
| `pre-tool-use` | `PreToolUse` | `PreToolUse` | `preToolUse` | `PreToolUse` | `preToolUse` | — | — |
| `post-tool-use` | `PostToolUse` | `PostToolUse` | `postToolUse` | `PostToolUse` | `postToolUse` | — | — |
| `user-prompt-submit` | `PreInvocation` | `UserPromptSubmit` | `userPromptSubmitted` | `UserPromptSubmit` | `userPromptSubmit` | — | — |
| `session-start` | `SessionStart` | `SessionStart` | `sessionStart` | `SessionStart` | `agentSpawn` | — | — |

---

## Adding a new agent

To add support for a new agent:

1. Add a variant to the `HookAgent` enum in `hook_schema.rs`.
2. Create an agent module (e.g., `hook_schema/newagent.rs`) implementing the `Agent` trait and the event-specific payload/output types.
3. Implement the `AgentHookPayload` and `AgentHookOutput` traits to convert between the agent's wire format and the internal `HookPayload`/`HookOutput` types.
4. Document the agent's hook registration locations and extension file layout in this page.
