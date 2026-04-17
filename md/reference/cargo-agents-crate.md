# `symposium crate`

Find crate sources and guidance.

## Usage

```bash
symposium crate <NAME> [--version <VERSION>]
symposium crate --list
```

## Behavior

When given a crate name, fetches the crate's source code and returns guidance including:

- Path to the extracted crate source
- Custom instructions from matching skill plugins
- Available skills that can be loaded

When `--list` is used (or no name is given), lists all workspace dependency crates with available skills.

This command is also available via the MCP server (`symposium mcp`).

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Crate name to get guidance for |
| `--version <VERSION>` | Version constraint (e.g., `1.0.3`, `^1.0`). Defaults to the workspace version or latest. |
| `--list` | List all workspace dependency crates with available skills |
