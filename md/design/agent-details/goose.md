# Goose Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Goose's extensibility surface.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Extensions](https://block.github.io/goose/docs/guides/using-extensions)
> · [Configuration](https://block.github.io/goose/docs/guides/config-files)
> · [GitHub repo](https://github.com/block/goose)

**Symposium registers no hooks for Goose.** Goose has lifecycle hooks of its own (a plugin can carry them in `hooks/hooks.json`), but symposium does not use them; it reaches Goose through MCP server registration.

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

Since symposium registers no hooks for Goose, it integrates exclusively via
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

- **User-level**: `~/.config/goose/config.yaml` (`$GOOSE_PATH_ROOT/config/config.yaml`
  when that variable is set, else `$XDG_CONFIG_HOME/goose/config.yaml` when
  that one is)
- **Project-level**: none - Goose reads only the user-level file, so
  project-scoped registration resolves there.

`GOOSE_PATH_ROOT` moves that file: when it is set, Goose reads
`$GOOSE_PATH_ROOT/config/config.yaml`, whatever `XDG_CONFIG_HOME` says, and an
empty value counts as unset. Symposium writes its MCP servers there too.
Verified against Goose 1.54.0 with a different MCP server in each candidate
file, by which one `goose run --text hi` started: the one under
`$GOOSE_PATH_ROOT` when both variables were set, and the `XDG_CONFIG_HOME` one
when `GOOSE_PATH_ROOT` was empty. Through `cargo agents sync` with the variable
set, a server left in `~/.config/goose/config.yaml` never started, while the
one symposium wrote under `$GOOSE_PATH_ROOT` started and was offered to the
model as a tool.

Two ways to check a change without a configured LLM provider: `goose recipe
validate` on a recipe holding the entry (it deserializes the same
`ExtensionConfig`), and `goose run --text hi`, whose startup warnings name each
extension it tried to launch. `goose info -v` only echoes the raw YAML, so it
cannot tell an accepted entry from a rejected one.

Registration is idempotent — if the entry already exists with the correct
values, no changes are made. Stale entries are updated in place.

## Plugin Delivery

A plugin enabled with `use --global` reaches Goose as a copy of the compiled
plugin directory in `~/.agents/plugins/<plugin>/`, where `goose plugin install`
puts the plugins it clones (`$GOOSE_PATH_ROOT/.agents/plugins/<plugin>/` when
that variable is set; `XDG_CONFIG_HOME` moves `config.yaml` but not plugins).
Goose loads every directory there in every project, reading the root
`plugin.json` and the skills under `skills/`; `.claude-plugin/` and the
`.symposium` marker are ignored.

Skills keep the name their `SKILL.md` gives them: Goose adds the `<plugin>:`
prefix only to plugins it installs itself, by rewriting each `SKILL.md`. Goose
records each plugin directory it finds under `plugins.<absolute path>.enabled`
in its `config.yaml`. Symposium never edits that map, so a user's `false`
stays, and an entry left after a removal is harmless. Goose 1.55.0 also searches
`~/.claude/skills/` two levels deep, so with Claude Code configured it loads the
same skills from Claude Code's copy (`~/.claude/skills/<plugin>/skills/<skill>/`),
and turning the plugin off in Goose then has no effect.

Verified against Goose 1.53.0 through `cargo agents sync`, by asking the agent
(`goose skills list`, and the system prompt a `goose session` sends to a local
model endpoint):

- The skill is listed from `~/.agents/plugins/<plugin>/skills/<skill>` after
  the sync.
- An edit at the same version reaches the next listing.
- Removing the directory removes the skill, with no error.
- Goose looks for plugins again on every turn, so a plugin a sync installs
  while a session runs is offered from the next message.
- A per-skill copy of the same skill in `.agents/skills/` is listed instead of
  the plugin's.
- `GOOSE_PATH_ROOT` moves the folder symposium writes and Goose reads.

Goose also loads `<working directory>/.agents/plugins/<plugin>/` as a project
plugin, with no trust prompt; symposium does not write project-scoped plugins.
