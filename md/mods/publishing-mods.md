# Publishing Agent Mods

## Publishing to crates.io

The simplest way to distribute an agent mod is to publish it to crates.io. Symposium can install mods directly from crates.io using `cargo binstall` (for pre-built binaries) or `cargo install` (building from source).

To make your agent mod installable:

1. Publish your crate to crates.io as usual
2. Include a binary target that speaks MCP over stdio
3. Optionally add `[package.metadata.symposium]` for configuration (see [Creating Mods](./creating-mods.md))

That's it. Users can reference your mod by crate name, and crate authors can recommend it in their Cargo.toml (see [Recommending Mods](./recommending-mods.md)).

## Publishing to the ACP Registry (optional)

The [ACP Registry](https://github.com/agentclientprotocol/registry) is a curated catalog of mods with broad applicability. Publishing here is appropriate for:

- **General-purpose mods** like Sparkle (AI collaboration identity) that help across all projects
- **Language/framework mods** that benefit many projects
- **Tool integrations** that aren't tied to a specific library

For **crate-specific mods** (e.g., a mod that helps with a particular library), crates.io distribution with Cargo.toml recommendations is more appropriate. Users of that library will discover the mod through the recommendation system.

### Submitting to the Registry

1. Fork the registry repository
2. Create a directory for your mod: `my-mod/`
3. Add `mod.json`:

```json
{
  "id": "my-mod",
  "name": "My Mod",
  "version": "0.1.0",
  "description": "General-purpose mod for X",
  "repository": "https://github.com/you/my-mod",
  "license": "MIT",
  "distribution": {
    "cargo": {
      "crate": "my-mod"
    }
  }
}
```

4. Submit a pull request

### Distribution Types

Mods in the registry can specify different distribution methods:

| Type | Example | Description |
|------|---------|-------------|
| `cargo` | `{ "crate": "my-mod" }` | Rust crate from crates.io |
| `npx` | `{ "package": "@org/mod" }` | npm package |
| `binary` | Platform-specific archives | Pre-built binaries |

For Rust crates, `cargo` distribution is recommended - it leverages the existing crates.io infrastructure.
