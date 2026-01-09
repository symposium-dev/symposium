# Publishing Extensions

The [ACP Registry](https://github.com/agentclientprotocol/registry) is the central catalog of agents and extensions. Publishing here makes your extension available to all ACP-compatible tools.

## Manual Submission

1. Fork the registry repository
2. Create a directory for your extension: `my-extension/`
3. Add `extension.json`:

```json
{
  "id": "my-extension",
  "name": "My Extension",
  "version": "0.1.0",
  "description": "Help agents work with MyLibrary",
  "repository": "https://github.com/you/my-extension",
  "license": "MIT",
  "distribution": {
    "cargo": {
      "crate": "my-extension"
    }
  }
}
```

4. Submit a pull request

## Distribution Types

Extensions can be distributed via:

| Type | Example | Description |
|------|---------|-------------|
| `cargo` | `{ "crate": "my-ext" }` | Rust crate from crates.io |
| `npx` | `{ "package": "@org/ext" }` | npm package |
| `binary` | Platform-specific archives | Pre-built binaries |

For Rust crates, the `cargo` distribution is simplest - Symposium handles installation via `cargo binstall` or `cargo install`.

## GitHub Action (Coming Soon)

A GitHub Action will automate this process - it reads your Cargo.toml metadata and submits to the registry on release.
