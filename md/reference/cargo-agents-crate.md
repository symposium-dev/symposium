# `symposium crate`

Find crate sources and guidance.

## Usage

```bash
symposium crate <NAME> [--version <VERSION>]
```

## Behavior

Fetches the crate's source code and returns guidance including:

- Path to the extracted crate source
- Custom instructions from matching skill plugins
- Available skills that can be loaded

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Crate name to get guidance for |
| `--version <VERSION>` | Version constraint (e.g., `1.0.3`, `^1.0`). Defaults to the workspace version or latest. |
