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

The MCP server entry is added under `extensions` in the YAML config. Goose's
schema has its own vocabulary - a `type` discriminant, the binary under `cmd`
(not `command`), and env as an `envs` map:

```yaml
extensions:
  symposium:
    name: symposium
    type: stdio
    cmd: /path/to/cargo-agents
    args: [mcp]
    enabled: true
    envs:
      TOKEN: abc
```

A remote server is `type: streamable_http` with the endpoint under `uri`. There
is no `sse` variant - the accepted set is `stdio`, `builtin`, `platform`,
`streamable_http`, `frontend`, `inline_python`, which `goose recipe validate`
will list back at you for a wrong one:

```yaml
extensions:
  remote:
    name: remote
    type: streamable_http
    uri: https://example.com/mcp
    enabled: true
    headers:
      Authorization: Bearer t
```

- **User-level**: `~/.config/goose/config.yaml`
- **Project-level**: none - Goose reads only the user-level file, so
  project-scoped registration resolves there.

Two ways to check a change without a configured LLM provider: `goose recipe
validate` on a recipe holding the entry (it deserializes the same
`ExtensionConfig`), and `goose run --text hi`, whose startup warnings name each
extension it tried to launch. `goose info -v` only echoes the raw YAML, so it
cannot tell an accepted entry from a rejected one.

Registration is idempotent — if the entry already exists with the correct
values, no changes are made. Stale entries are updated in place.
