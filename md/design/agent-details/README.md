# Agent details

> **Disclaimer:** These documents reflect our current understanding of each agent's hook system
> and extensibility surface. They are maintained as working references for symposium development,
> not as a substitute for each project's official documentation. Details may be outdated or
> incomplete — always consult the primary sources linked in each agent's page.

For each agent it supports, symposium needs to know:

1. **Hook registration** — where and how to write config so the agent calls `cargo-agents hook`
2. **Hook I/O protocol** — event names, input/output field names, exit code semantics
3. **Extension installation** — where skill files go (project and global)
4. **Custom instructions** — where the agent reads project-level instructions

The tables below summarize the answers for each agent. Individual agent pages contain the full reference. A **?** indicates information we have not yet documented.

## Hook registration

| Agent | Project config path | Global config path | Format |
|---|---|---|---|
| [Antigravity CLI](./antigravity-cli.md) | `.agents/hooks.json` | `~/.gemini/config/hooks.json` | JSON, named entries per hook |
| [Claude Code](./claude-code.md) | `.claude/settings.json` | `~/.claude/settings.json` | JSON, `hooks` key with matcher groups |
| [GitHub Copilot](./copilot.md) | `.github/hooks/*.json` | `~/.copilot/settings.json` | JSON, `version: 1` with `hooks` key |
| [Codex CLI](./codex-cli.md) | `.codex/hooks.json` | `~/.codex/hooks.json` | JSON, `hooks` key with matcher groups |
| [Kiro](./kiro.md) | `.kiro/agents/*.json` | `~/.kiro/agents/*.json` | JSON, `hooks` key in agent config |
| [OpenCode](./opencode.md) | `.opencode/plugins/` | `~/.config/opencode/plugins/` | JS/TS plugins (not shell hooks) |
| [Goose](./goose.md) | *(no hooks)* | *(no hooks)* | N/A |
| [Pi](./pi.md) | `.pi/extensions/symposium.ts` | `~/.pi/agent/extensions/symposium.ts` | TypeScript extension |

### Command field

| Agent | Command field | Platform-specific? |
|---|---|---|
| Antigravity CLI | `command` | No |
| Claude Code | `command` | No |
| GitHub Copilot | `bash` / `powershell` | Yes |
| Codex CLI | `command` | No |
| Kiro | `command` | No |
| OpenCode | N/A (JS function) | N/A |
| Goose | N/A | N/A |
| Pi extension | `execFile("cargo-agents", ["hook", "pi", event])` | No |

### Timeout defaults

| Agent | Default timeout | Unit |
|---|---|---|
| Antigravity CLI | 30 | seconds (`timeout`) |
| Claude Code | 600 | seconds |
| GitHub Copilot | 30 | seconds (`timeoutSec`) |
| Codex CLI | 600 | seconds (`timeout` or `timeoutSec`) |
| Kiro | 30,000 | milliseconds (`timeout_ms`) |
| OpenCode | 60,000 | milliseconds (community hooks plugin) |
| Goose | N/A | N/A |
| Pi extension | 30 | seconds |

## Event names

Symposium registers hooks for four events on every agent with shell hooks, plus `stop` on Antigravity and Claude Code. Each agent uses different names and casing conventions; *not registered* marks an event the agent has but symposium does not hook.

| Symposium event | Antigravity CLI | Claude Code | Copilot | Codex CLI | Kiro CLI | OpenCode | Goose |
|---|---|---|---|---|---|---|---|
| pre-tool-use | `PreToolUse` | `PreToolUse` | `preToolUse` | `PreToolUse` | `preToolUse` | `tool.execute.before` | N/A |
| post-tool-use | `PostToolUse` | `PostToolUse` | `postToolUse` | `PostToolUse` | `postToolUse` | `tool.execute.after` | N/A |
| user-prompt-submit | `PreInvocation` | `UserPromptSubmit` | `userPromptSubmitted` | `UserPromptSubmit` | `userPromptSubmit` | `message.updated` (filter by role) | N/A |
| session-start | `SessionStart` | `SessionStart` | `sessionStart` | `SessionStart` | `agentSpawn` | `session.created` | N/A |
| stop | `Stop` | `Stop` | *not registered* (`agentStop`) | *not registered* (`Stop`) | *not registered* (`stop`) | N/A | N/A |

Pi uses extension events rather than shell hooks. Its mapping is documented in
[Pi integration](./pi.md#event-mapping-and-wire-format).

### Blocking support

Not all events can block the action in all agents.

| Agent | Pre-tool-use can block? | Post-tool-use can block? | User-prompt can block? | Session-start can block? |
|---|---|---|---|---|
| Antigravity CLI | Yes | No | No | No |
| Claude Code | Yes | No | Yes (exit 2) | No |
| GitHub Copilot | Yes | No | No | No |
| Codex CLI | Yes | Yes (`continue: false`) | Yes (`continue: false`) | Yes (`continue: false`) |
| Kiro | Yes (exit 2) | No | No | No |
| OpenCode | Yes (throw Error) | No | No (observe only) | No (observe only) |
| Goose | N/A | N/A | N/A | N/A |
| Pi | Yes | No | No | No |

## Hook I/O protocol

### Input fields (pre-tool-use)

| Agent | Tool name field | Tool args field | Session/context fields |
|---|---|---|---|
| Antigravity CLI | `toolCall.name` | `toolCall.args` (object) | `conversationId`, `workspacePaths` (array), `stepIdx` |
| Claude Code | `tool_name` | `tool_input` (object) | `session_id`, `cwd`, `hook_event_name` |
| GitHub Copilot | `toolName` | `toolArgs` (JSON **string**) | `timestamp`, `cwd` |
| Codex CLI | `tool_name` | `tool_input` (object) | `session_id`, `cwd`, `hook_event_name`, `model` |
| Kiro | `tool_name` | `tool_input` (object) | `hook_event_name`, `cwd` |
| OpenCode | `tool` | `args` (mutable output object) | `sessionID`, `callID` |
| Goose | N/A | N/A | N/A |
| Pi bridge | `tool_name` | `tool_input` (object) | `session_id`, `cwd` |

### Output structure (pre-tool-use)

| Agent | Permission decision field | Decision values | Modified input field | Nesting |
|---|---|---|---|---|
| Antigravity CLI | `decision` (**required**) | allow, deny, ask, force_ask | `overwrite` | flat |
| Claude Code | `permissionDecision` | allow, deny, ask, defer | `updatedInput` | nested in `hookSpecificOutput` |
| GitHub Copilot | `permissionDecision` | allow, deny, ask | `modifiedArgs` | flat |
| Codex CLI | `decision` or `permissionDecision` | block/deny | *(not yet implemented)* | flat or nested `hookSpecificOutput` |
| Kiro | *(exit code only)* | exit 0 = allow, exit 2 = block | *(not supported)* | N/A |
| OpenCode | *(throw to block)* | allow (return) / deny (throw) | mutate `output.args` | JS mutation |
| Goose | N/A | N/A | N/A | N/A |
| Pi bridge | `decision` | allow, deny | `updatedInput` | flat |

### Exit codes

All shell-based agents use the same convention (where applicable):

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed as JSON |
| `2` | Block/deny; stderr used as reason |
| Other | Non-blocking warning, action proceeds |

**Exceptions**: Copilot uses exit 0 = allow, non-zero = deny (no special meaning for exit 2). OpenCode uses JS exceptions, not exit codes. **Antigravity ignores exit codes entirely** — only stdout decides, and on `PreToolUse` an object without a valid `decision` (`{}` included) is a denial.

## Extension installation

### Skill file paths

| Agent | Project skills path | Global skills path |
|---|---|---|
| Antigravity CLI | `.agents/skills/<name>/SKILL.md` | `~/.gemini/config/skills/<name>/SKILL.md` |
| Claude Code | `.claude/skills/<name>/SKILL.md` | `~/.claude/skills/<name>/SKILL.md` |
| GitHub Copilot | `.agents/skills/<name>/SKILL.md` | *(none)* |
| Codex CLI | `.agents/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/SKILL.md` |
| Kiro | `.kiro/skills/<name>/SKILL.md` | `~/.kiro/skills/<name>/SKILL.md` |
| OpenCode | `.agents/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/SKILL.md` |
| Goose | *(N/A — uses MCP extensions)* | *(N/A)* |
| Pi | `.agents/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/SKILL.md` |

Symposium uses the vendor-neutral `.agents/skills/` path whenever the agent supports it, falling back to agent-specific paths (e.g., `.claude/skills/`, `.kiro/skills/`) when required. Codex CLI and OpenCode also support `.agents/skills/` natively.

### Custom instructions

| Agent | Project instructions | Global instructions |
|---|---|---|
| Antigravity CLI | `AGENTS.md`, `GEMINI.md`, `.agents/rules/*.md` | *(none)* |
| Claude Code | `CLAUDE.md`, `.claude/CLAUDE.md` | `~/.claude/CLAUDE.md` |
| GitHub Copilot | `.github/copilot-instructions.md`, `AGENTS.md` | `~/.copilot/copilot-instructions.md` |
| Codex CLI | `AGENTS.md` (each dir level) | `~/.codex/AGENTS.md` |
| Kiro | `.kiro/steering/*.md`, `AGENTS.md` | `~/.kiro/steering/*.md` |
| OpenCode | `AGENTS.md`, `CLAUDE.md` | `~/.config/opencode/AGENTS.md` |
| Goose | `.goosehints`, `AGENTS.md` | `~/.config/goose/.goosehints` |
| Pi | `AGENTS.md`, `CLAUDE.md` | `~/.pi/agent/AGENTS.md` |

## MCP server configuration

Relevant if symposium exposes functionality via MCP.

An agent's MCP file is usually **not** the file its hooks live in, and the
per-agent entry shapes differ more than they look. `Agent::mcp_config_path`
encodes this table; every row marked verified was confirmed by asking the tool
(`<tool> mcp list` reporting an entry symposium wrote, and `<tool> mcp add`
showing which file it chooses), because a wrong guess here fails silently -
symposium reports success for a file the agent never reads.

| Agent | Project scope | User scope | Entry shape | Verified |
|---|---|---|---|---|
| Claude Code | `<project>/.mcp.json` | `~/.claude.json` | `mcpServers.<name>` = `{command, args}` | yes |
| Antigravity CLI | `<project>/.agents/mcp_config.json` | `~/.gemini/config/mcp_config.json` | `mcpServers.<name>` = `{command, args}` | yes (`agy mcp add`) |
| OpenCode | `<project>/opencode.json` | `~/.config/opencode/opencode.json` | `mcp.<name>` = `{type: "local", command: [bin, ...args], enabled, environment}` | yes |
| Codex CLI | *(none - user scope only)* | `~/.codex/config.toml` | `[mcp_servers.<name>]` = `command`, `args` | yes |
| GitHub Copilot CLI | *(none - user scope only)* | `~/.copilot/mcp-config.json` | `mcpServers.<name>` = `{command, args}` | yes |
| Kiro | `.kiro/settings/mcp.json` | `~/.kiro/settings/mcp.json` | `mcpServers.<name>` = `{command, args}` | no (GUI only) |
| Goose | *(none - user scope only)* | `~/.config/goose/config.yaml` | `extensions.<name>` = `{name, type: stdio, cmd, args, enabled, envs}` | yes |
| Pi | `.pi/mcp.json` | `~/.pi/agent/mcp.json` | `mcpServers.<name>` = `{command, args}` or `{url, headers}` | yes (`pi mcp list`) |

Notes that cost real debugging time:

- Claude Code ignores `mcpServers` in `settings.json` at both scopes - that file
  is hooks only. See [Claude Code](./claude-code.md#mcp-server-registration).
- The Copilot **CLI** requires the `mcpServers` wrapper; entries written bare at
  the top level make it reject the whole file (`mcpServers: Required`), taking
  the user's own servers down with it. `.vscode/mcp.json` belongs to the VS Code
  extension, a different product symposium does not currently target.
- OpenCode rejects the entire config file for a wrong entry shape, so its
  serializer is separate: the command is one array, and env vars go under
  `environment` (an `env` key parses but never reaches the child).
- Codex, Copilot CLI and Goose have no project-level MCP config, so project
  scope resolves to their user-level file rather than to a file nobody reads.
- Antigravity keeps MCP in its own `mcp_config.json`, not in the `hooks.json`
  beside it, so a broken hooks file cannot take MCP down with it. `agy mcp add`
  has no scope flag and always writes the user-level file.
- Goose uses `cmd`, not `command`, and requires a `type`; the nested
  `provider: mcp` / `config:` form it once got is rejected outright. Remote is
  `type: streamable_http` with `uri` - Goose has no `sse` variant.
- `env`/`headers` must be **maps**. ACP models them as `[{name, value}]` pairs,
  and a list is skipped or rejected - silently, for an entry that differs from a
  working one only by carrying env. Codex takes env as a TOML table, Goose as
  `envs`, OpenCode as `environment`, everyone else as `env`.
- The Copilot CLI also needs an explicit `type` (`local`/`http`/`sse`), or a
  remote entry never appears.
