# Goose

Config name: `goose`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |

## Plugins

A plugin enabled with `cargo agents use --global` is installed for you as a plugin directory:

| Scope | Path |
|-------|------|
| Global | `~/.agents/plugins/<plugin>/` (`$GOOSE_PATH_ROOT/.agents/plugins/<plugin>/` when set) |

Goose loads it in every project, under the skills' own names. Symposium leaves the plugin entries Goose records in its `config.yaml` alone, so turning one off there stays in effect. With Claude Code also configured, Goose loads the same skills from Claude Code's copy in `~/.claude/skills/<plugin>/`, so turning the plugin off in Goose has no effect. A plugin installed while a session runs is available from the next message.

## Hooks

**Not registered.** Goose has hooks of its own, but symposium does not register any for it, so `cargo agents hook` is never called by this agent.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.config/goose/config.yaml` (`$GOOSE_PATH_ROOT/config/config.yaml` when set) | `extensions.<name>` |

Goose reads no project-level MCP file, so a project-scoped registration goes to the global file.
