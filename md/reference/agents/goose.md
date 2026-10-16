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

Goose loads it in every project. Its skills keep their own names, since Goose adds the `<plugin>:` prefix only to plugins it installs itself, so a skill of the same name elsewhere, such as a per-project copy, is the one Goose uses. Goose remembers each plugin it finds in its `config.yaml`; turning one off there is a choice symposium leaves alone. A plugin installed while a session runs is available from the next message.

## Hooks

**Not registered.** Goose has hooks of its own, but symposium does not register any for it, so `cargo agents hook` is never called by this agent.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.config/goose/config.yaml` | `extensions.<name>` |

Goose reads no project-level MCP file, so a project-scoped registration goes to the global file.
