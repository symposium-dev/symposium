# Publishing Agent Extensions

## Publishing to crates.io

The simplest way to distribute an agent extension is to publish it to crates.io. Symposium can install extensions directly from crates.io using `cargo binstall` (for pre-built binaries) or `cargo install` (building from source).

To make your agent extension installable:

1. Publish your crate to crates.io as usual
2. Include a binary target that speaks MCP over stdio
3. Optionally add `[package.metadata.symposium]` for configuration (see [Creating Extensions](./creating-extensions.md))

That's it. Users can reference your extension by crate name, and crate authors can recommend it in their Cargo.toml (see [Recommending Extensions](./recommending-extensions.md)).

## Publishing to the ACP Registry (optional)

The [ACP Registry](https://github.com/agentclientprotocol/registry) is a curated catalog of extensions with broad applicability. Publishing here is appropriate for:

- **General-purpose extensions** like Sparkle (AI collaboration identity) that help across all projects
- **Language/framework extensions** that benefit many projects
- **Tool integrations** that aren't tied to a specific library

For **crate-specific extensions** (e.g., an extension that helps with a particular library), crates.io distribution with Cargo.toml recommendations is more appropriate. Users of that library will discover the extension through the recommendation system.

### Submitting to the Registry

1. Fork the registry repository
2. Create a directory for your extension: `my-extension/`
3. Add `extension.json`:

```json
{
  "id": "my-extension",
  "name": "My Extension",
  "version": "0.1.0",
  "description": "General-purpose extension for X",
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

### Distribution Types

Extensions in the registry can specify different distribution methods:

| Type | Example | Description |
|------|---------|-------------|
| `cargo` | `{ "crate": "my-ext" }` | Rust crate from crates.io |
| `npx` | `{ "package": "@org/ext" }` | npm package |
| `binary` | Platform-specific archives | Pre-built binaries |

For Rust crates, `cargo` distribution is recommended - it leverages the existing crates.io infrastructure.
