# `symposium crate-info`

Find crate sources and guidance.

This is an agent-facing command, listed under "Commands for agents" in `symposium --help`. Its output format and exit codes may change in future releases.

## Usage

```bash
symposium crate-info <NAME> [--version <VERSION>]
```

## Behavior

Fetches the crate's source code and returns:

- Path to the extracted crate source
- Available skills for the crate

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Crate name to get guidance for |
| `--version <VERSION>` | Version constraint (e.g., `1.0.3`, `^1.0`). Defaults to the workspace version or latest. |
