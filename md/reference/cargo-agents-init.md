# `cargo agents init`

Initialize Symposium for the current user, the current project, or both.

## Usage

```bash
cargo agents init [OPTIONS]
```

## Behavior

With no flags, `init` does whatever hasn't been done yet:

1. If no user-wide configuration exists (`~/.symposium/config.toml`), runs user setup.
2. If no project configuration exists (`.symposium/config.toml`), offers to set up the project.
3. Runs the appropriate syncs: if the project was set up (or already existed), runs `cargo agents sync` (both `--workspace` and `--agent`). If only user setup was performed, runs `cargo agents sync --agent` to register global hooks.

The same applies when both `--user` and `--project` are specified explicitly.

### `--user`

Set up user-wide configuration only. Can be run from any directory.

Prompts for:

- Which agent you use (e.g., Claude Code, Cursor)

Writes `~/.symposium/config.toml` and, where applicable, registers a global hook so your agent automatically picks up project extensions on startup.

### `--project`

Set up the current project only. Must be run from within a Rust workspace.

Prompts for:

- Whether to set a project-level agent override (default: use each developer's own preference)

Scans workspace dependencies, discovers available extensions, and generates `.symposium/config.toml`. Runs `cargo agents sync` afterward.

## Options

| Flag | Description |
|------|-------------|
| `--user` | Set up user-wide configuration only |
| `--project` | Set up project configuration only |
