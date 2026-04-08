# `symposium sync`

Synchronize the project configuration and agent setup.

## Usage

```bash
symposium sync [OPTIONS]
```

## Behavior

By default, `sync` performs both steps:

1. **Workspace sync** (`--workspace`) — scans the current workspace dependencies and updates `.symposium/config.toml`:
   - Extensions for new dependencies are added, defaulting to the resolved `sync-default` value.
   - Entries for removed dependencies are cleaned up.
   - Existing on/off choices are preserved.

2. **Agent sync** (`--agent`) — reads `.symposium/config.toml` and installs the enabled extensions into the locations your agent expects (e.g., `.claude/skills/` for Claude Code).

## Options

| Flag | Description |
|------|-------------|
| `--workspace` | Only update `.symposium/config.toml` from workspace dependencies |
| `--agent` | Only install enabled extensions into the agent's directories |
| `--set-agent <name>` | Set or change the project-level agent override |
