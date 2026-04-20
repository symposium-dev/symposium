# `cargo agents crate-info`

Find crate sources and guidance.

## Usage

```bash
cargo agents crate-info <NAME> [--version <VERSION>]
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
