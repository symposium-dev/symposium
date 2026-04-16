# Goose Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Goose's extensibility surface.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Extensions](https://block.github.io/goose/docs/guides/using-extensions)
> · [Configuration](https://block.github.io/goose/docs/guides/config-files)
> · [GitHub repo](https://github.com/block/goose)

**Goose does not implement lifecycle hooks.** There are no shell-command or programmatic interception points for tool execution, session start/stop, or prompt submission. No hooks.json equivalent. No JSON stdin/stdout protocol.

## What Goose Offers Instead

### MCP Extensions

The primary extensibility mechanism. Extensions are MCP servers (stdio or HTTP) that expose new tools, resources, and prompts. Configured in `~/.config/goose/config.yaml` under `extensions:`. Built-in extensions include Developer (shell, file editing), Computer Controller, Memory, and Todo. Custom extensions are standard MCP servers built in Python, TypeScript, or Kotlin. Extensions **add capabilities** but cannot intercept or modify existing tool behavior.

### Permission Modes

The closest analog to hook-based control flow. Static configuration, not programmable logic.

| Mode | Behavior |
|---|---|
| `auto` | Tools execute without approval (default) |
| `approve` | Every tool call requires manual confirmation |
| `smart_approve` | AI risk assessment auto-approves low-risk, prompts for high-risk |
| `chat` | No tool use |

Per-tool permissions can be set to Always Allow, Ask Before, or Never Allow.

### Goosehints / AGENTS.md

Instruction files injected into the system prompt. Influence LLM behavior through prompting, not deterministic interception.

| File | Scope |
|---|---|
| `~/.config/goose/.goosehints` | Global |
| `.goosehints` (project root) | Project |
| `AGENTS.md` | Project |

### GOOSE_TERMINAL Environment Variable

Shell scripts can detect whether they're running under Goose and alter behavior (e.g., wrapping `git` to block `git commit`). This is a shell-level workaround, not a Goose-native mechanism.

### Other Mechanisms

- `.gooseignore` — gitignore-style file access restriction
- Recipes — YAML workflow packages
- Custom slash commands
- Subagents
- ACP integration
- Tool Router — internal optimization for tool selection

## MCP Server Registration

Since Goose has no lifecycle hooks, symposium integrates exclusively via
MCP server registration. Symposium registers itself as an extension in
the Goose config file.

### Configuration structure

The MCP server entry is added under `extensions` in the YAML config:

```yaml
extensions:
  symposium:
    provider: mcp
    config:
      command: /path/to/symposium
      args: [mcp]
```

- **Project-level**: `.goose/config.yaml`
- **User-level**: `~/.config/goose/config.yaml`

Registration is idempotent — if the entry already exists with the correct
values, no changes are made. Stale entries are updated in place.
