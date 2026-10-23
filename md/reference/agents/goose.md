# Goose

Config name: `goose`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |

## Hooks

**Not registered.** Goose has hooks of its own, but symposium does not register any for it, so `cargo agents hook` is never called by this agent.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.config/goose/config.yaml` (`$GOOSE_PATH_ROOT/config/config.yaml` when set) | `extensions.<name>` |

Goose reads no project-level MCP file, so a project-scoped registration goes to the global file.
