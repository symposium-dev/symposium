# Agent details

For each agent it supports, symposium needs to know:

1. **Hook registration** — where and how to write config so the agent calls `symposium hook`
2. **Hook I/O protocol** — event names, input/output field names, exit code semantics
3. **Extension installation** — where skill files go (project and global)
4. **Custom instructions** — where the agent reads project-level instructions

The tables below summarize the answers for each agent. Individual agent pages contain the full reference. A **?** indicates information we have not yet documented.

## Hook registration

| Agent | Project config path | Global config path | Format |
|---|---|---|---|
| [Claude Code](./claude-code-hooks.md) | `.claude/settings.json` | `~/.claude/settings.json` | JSON, `hooks` key with matcher groups |
| [GitHub Copilot](./copilot-hooks.md) | `.github/hooks/*.json` | `~/.copilot/config.json` | JSON, `version: 1` with `hooks` key |
| [Gemini CLI](./gemini-cli-hooks.md) | `.gemini/settings.json` | `~/.gemini/settings.json` | JSON, `hooks` key with matcher groups |
| [Codex CLI](./codex-cli-hooks.md) | `.codex/hooks.json` | `~/.codex/hooks.json` | JSON, `hooks` key with matcher groups |
| [Kiro](./kiro-hooks.md) | `.kiro/agents/*.json` | `~/.kiro/agents/*.json` | JSON, `hooks` key in agent config |
| [OpenCode](./opencode-hooks.md) | `.opencode/plugins/` | `~/.config/opencode/plugins/` | JS/TS plugins (not shell hooks) |
| [Goose](./goose-hooks.md) | *(no hooks)* | *(no hooks)* | N/A |

### Command field

| Agent | Command field | Platform-specific? |
|---|---|---|
| Claude Code | `command` | No |
| GitHub Copilot | `bash` / `powershell` | Yes |
| Gemini CLI | `command` | No |
| Codex CLI | `command` | No |
| Kiro | `command` | No |
| OpenCode | N/A (JS function) | N/A |
| Goose | N/A | N/A |

### Timeout defaults

| Agent | Default timeout | Unit |
|---|---|---|
| Claude Code | 600 | seconds |
| GitHub Copilot | 30 | seconds (`timeoutSec`) |
| Gemini CLI | 60,000 | milliseconds (`timeout`) |
| Codex CLI | 600 | seconds (`timeout` or `timeoutSec`) |
| Kiro | 30,000 | milliseconds (`timeout_ms`) |
| OpenCode | 60,000 | milliseconds (community hooks plugin) |
| Goose | N/A | N/A |

## Event names

Symposium registers hooks for four events. Each agent uses different names and casing conventions.

| Symposium event | Claude Code | Copilot | Gemini CLI | Codex CLI | Kiro CLI | OpenCode | Goose |
|---|---|---|---|---|---|---|---|
| pre-tool-use | `PreToolUse` | `preToolUse` | `BeforeTool` | `PreToolUse` | `preToolUse` | `tool.execute.before` | N/A |
| post-tool-use | `PostToolUse` | `postToolUse` | `AfterTool` | `PostToolUse` | `postToolUse` | `tool.execute.after` | N/A |
| user-prompt-submit | `UserPromptSubmit` | `userPromptSubmitted` | `BeforeAgent` | `UserPromptSubmit` | `userPromptSubmit` | `message.updated` (filter by role) | N/A |
| session-start | `SessionStart` | `sessionStart` | `SessionStart` | `SessionStart` | `agentSpawn` | `session.created` | N/A |

### Blocking support

Not all events can block the action in all agents.

| Agent | Pre-tool-use can block? | Post-tool-use can block? | User-prompt can block? | Session-start can block? |
|---|---|---|---|---|
| Claude Code | Yes | No | Yes (exit 2) | No |
| GitHub Copilot | Yes | No | No | No |
| Gemini CLI | Yes | Yes (block result) | Yes (deny discards message) | No |
| Codex CLI | Yes | Yes (`continue: false`) | Yes (`continue: false`) | Yes (`continue: false`) |
| Kiro | Yes (exit 2) | No | No | No |
| OpenCode | Yes (throw Error) | No | No (observe only) | No (observe only) |
| Goose | N/A | N/A | N/A | N/A |

## Hook I/O protocol

### Input fields (pre-tool-use)

| Agent | Tool name field | Tool args field | Session/context fields |
|---|---|---|---|
| Claude Code | `tool_name` | `tool_input` (object) | `session_id`, `cwd`, `hook_event_name` |
| GitHub Copilot | `toolName` | `toolArgs` (JSON **string**) | `timestamp`, `cwd` |
| Gemini CLI | `tool_name` | `tool_input` (object) | `session_id`, `cwd`, `hook_event_name`, `timestamp` |
| Codex CLI | `tool_name` | `tool_input` (object) | `session_id`, `cwd`, `hook_event_name`, `model` |
| Kiro | `tool_name` | `tool_input` (object) | `hook_event_name`, `cwd` |
| OpenCode | `tool` | `args` (mutable output object) | `sessionID`, `callID` |
| Goose | N/A | N/A | N/A |

### Output structure (pre-tool-use)

| Agent | Permission decision field | Decision values | Modified input field | Nesting |
|---|---|---|---|---|
| Claude Code | `permissionDecision` | allow, deny, ask, defer | `updatedInput` | nested in `hookSpecificOutput` |
| GitHub Copilot | `permissionDecision` | allow, deny, ask | `modifiedArgs` | flat |
| Gemini CLI | `decision` | allow, deny | `tool_input` | nested in `hookSpecificOutput` |
| Codex CLI | `decision` or `permissionDecision` | block/deny | *(not yet implemented)* | flat or nested `hookSpecificOutput` |
| Kiro | *(exit code only)* | exit 0 = allow, exit 2 = block | *(not supported)* | N/A |
| OpenCode | *(throw to block)* | allow (return) / deny (throw) | mutate `output.args` | JS mutation |
| Goose | N/A | N/A | N/A | N/A |

### Exit codes

All shell-based agents use the same convention (where applicable):

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed as JSON |
| `2` | Block/deny; stderr used as reason |
| Other | Non-blocking warning, action proceeds |

**Exceptions**: Copilot uses exit 0 = allow, non-zero = deny (no special meaning for exit 2). OpenCode uses JS exceptions, not exit codes.

## Extension installation

### Skill file paths

| Agent | Project skills path | Global skills path |
|---|---|---|
| Claude Code | `.claude/skills/<name>/SKILL.md` | `~/.claude/skills/<name>/SKILL.md` |
| GitHub Copilot | `.agents/skills/<name>/SKILL.md` | *(none)* |
| Gemini CLI | `.agents/skills/<name>/SKILL.md` | `~/.gemini/skills/<name>/SKILL.md` |
| Codex CLI | `.agents/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/SKILL.md` |
| Kiro | `.kiro/skills/<name>/SKILL.md` | `~/.kiro/skills/<name>/SKILL.md` |
| OpenCode | `.agents/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/SKILL.md` |
| Goose | *(N/A — uses MCP extensions)* | *(N/A)* |

Symposium uses the vendor-neutral `.agents/skills/` path whenever the agent supports it, falling back to agent-specific paths (e.g., `.claude/skills/`, `.kiro/skills/`) when required. Codex CLI and OpenCode also support `.agents/skills/` natively.

### Custom instructions

| Agent | Project instructions | Global instructions |
|---|---|---|
| Claude Code | `CLAUDE.md`, `.claude/CLAUDE.md` | `~/.claude/CLAUDE.md` |
| GitHub Copilot | `.github/copilot-instructions.md`, `AGENTS.md` | `~/.copilot/copilot-instructions.md` |
| Gemini CLI | `GEMINI.md` (walks up to `.git`) | `~/.gemini/GEMINI.md` |
| Codex CLI | `AGENTS.md` (each dir level) | `~/.codex/AGENTS.md` |
| Kiro | `.kiro/steering/*.md`, `AGENTS.md` | `~/.kiro/steering/*.md` |
| OpenCode | `AGENTS.md`, `CLAUDE.md` | `~/.config/opencode/AGENTS.md` |
| Goose | `.goosehints`, `AGENTS.md` | `~/.config/goose/.goosehints` |

## MCP server configuration

Relevant if symposium exposes functionality via MCP.

| Agent | MCP config location | Format |
|---|---|---|
| Claude Code | `.claude/settings.json` (`mcpServers` key) | JSON |
| GitHub Copilot | `.vscode/mcp.json` (VS Code), `~/.copilot/mcp-config.json` (CLI) | JSON |
| Gemini CLI | `.gemini/settings.json` (`mcpServers` key) | JSON |
| Codex CLI | `.codex/config.toml` / `~/.codex/config.toml` (`mcp_servers` key) | TOML |
| Kiro | `.kiro/settings/mcp.json`, `~/.kiro/settings/mcp.json` | JSON |
| OpenCode | `opencode.json` (`mcp` key) | JSON |
| Goose | `~/.config/goose/config.yaml` (`extensions` key) | YAML |
