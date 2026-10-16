# Goose Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Goose's extensibility surface.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Extensions](https://block.github.io/goose/docs/guides/using-extensions)
> · [Configuration](https://block.github.io/goose/docs/guides/config-files)
> · [Plugins](https://goose-docs.ai/docs/guides/context-engineering/plugins)
> · [GitHub repo](https://github.com/block/goose)

**Symposium registers no hooks for Goose.** Goose has lifecycle hooks now (a `hooks/hooks.json`, which a plugin can carry), but symposium does not use them yet: it reaches Goose through MCP server registration and [plugin delivery](#plugin-delivery).

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

Without hook registration, symposium integrates with Goose through MCP server
registration and plugin delivery. Symposium registers itself as an extension in
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

`GOOSE_PATH_ROOT` moves both places symposium writes: when it is set, Goose
reads `$GOOSE_PATH_ROOT/config/config.yaml`, whatever `XDG_CONFIG_HOME` says,
and its user plugins from `$GOOSE_PATH_ROOT/.agents/plugins/`; an empty value
counts as unset. Symposium writes its MCP servers and
[plugins](#plugin-delivery) there too. Verified against Goose 1.54.0 with a
different MCP server in each candidate file, by which one `goose run --text hi`
started: the one under `$GOOSE_PATH_ROOT` when both variables were set, and the
`XDG_CONFIG_HOME` one when `GOOSE_PATH_ROOT` was empty. Through `cargo agents
sync` with the variable set, a server left in `~/.config/goose/config.yaml`
never started, while the one symposium wrote under `$GOOSE_PATH_ROOT` started
and was offered to the model as a tool.

Two ways to check a change without a configured LLM provider: `goose recipe
validate` on a recipe holding the entry (it deserializes the same
`ExtensionConfig`), and `goose run --text hi`, whose startup warnings name each
extension it tried to launch. `goose info -v` only echoes the raw YAML, so it
cannot tell an accepted entry from a rejected one.

Registration is idempotent — if the entry already exists with the correct
values, no changes are made. Stale entries are updated in place.

## Plugin Delivery

A plugin enabled with `use --global` reaches Goose as a copy of the compiled
plugin directory in `~/.agents/plugins/<plugin>/`
(`$GOOSE_PATH_ROOT/.agents/plugins/<plugin>/` when that variable is set;
`XDG_CONFIG_HOME` moves Goose's `config.yaml` but not its plugins). Goose
discovers every directory there, reads the root `plugin.json` (`.claude-plugin/`
and the `.symposium` marker are ignored), and loads the skills under `skills/` in
every project. The directory name is the plugin's identity.

Its skills keep the name their `SKILL.md` gives them. Goose prefixes skill names
with `<plugin>:` only when `goose plugin install` installs a plugin, by rewriting
each `SKILL.md`; symposium copies skills unchanged, so they share the namespace
of standalone skills, where one name loads once.

Goose records each plugin directory it discovers in its `config.yaml`, under
`plugins.<absolute path>.enabled`. Symposium never edits that map: a user's
`enabled: false` there keeps the plugin hidden across syncs, and an entry left
after a plugin is removed is harmless.

Verified against Goose 1.53.0 through `cargo agents sync`, by asking the agent
(`goose skills list`, and the system prompt a `goose session` sends to a local
model endpoint):

- The plugin's skill is listed from `~/.agents/plugins/<plugin>/skills/<skill>`
  after the sync.
- An edit at the same version reaches the next listing: the copy is read in
  place.
- Removing the directory removes the skill, with no error.
- Goose discovers plugins again on every turn, so a plugin a sync installs while
  a session runs is offered from the next message.
- A per-skill copy in the project's `.agents/skills/` with the same name is
  listed instead of the plugin's skill.
- `GOOSE_PATH_ROOT` relocates the directory symposium writes and Goose reads.

### Project scope

Goose also loads `<working directory>/.agents/plugins/<plugin>/`, from the
working directory only, with no trust prompt. A project plugin wins over a user
plugin of the same name, and Antigravity reads the same folder. Symposium does
not write project-scoped plugins for Goose yet.
