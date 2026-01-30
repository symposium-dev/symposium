# Publishing Agent Mods

## Publishing to crates.io

The simplest way to distribute an agent mod is to publish it to crates.io. Symposium installs cargo mods using a two-step process:

1. **Try `cargo binstall`** - downloads pre-built binaries (fast)
2. **Fall back to `cargo install`** - builds from source (slower, but always works)

To make your agent mod installable:

1. Publish your crate to crates.io as usual
2. Include a binary target that speaks ACP over stdio
3. Optionally add `[package.metadata.symposium]` for configuration (see [Creating Mods](./creating-mods.md))

That's it. Users can reference your mod by crate name, and crate authors can recommend it in their Cargo.toml (see [Recommending Mods](./recommending-mods.md)).

### How Installation Works

When a user enables a mod distributed via cargo, Symposium:

1. Queries crates.io for the latest version and binary name(s)
2. Checks if the binary is already cached locally
3. If not cached, attempts `cargo binstall <crate>@<version>`
4. If binstall fails (not installed, or no pre-built binary), falls back to `cargo install`
5. Caches the binary to `<config_dir>/bin/<crate>/<version>/`

Old versions are automatically cleaned up when a new version is installed.

### Providing Pre-built Binaries

To make installation fast for your users, provide pre-built binaries that `cargo binstall` can download. See the [cargo-binstall documentation](https://github.com/cargo-bins/cargo-binstall) for details.

Common approaches:

- **GitHub Releases**: Upload binaries as release assets with names matching binstall's naming conventions
- **Quickinstall**: Register with [cargo-quickinstall](https://github.com/cargo-bins/cargo-quickinstall) for automated binary builds

If you don't provide pre-built binaries, `cargo install` will build from source. This works but takes longer.

### Cargo.toml Metadata

Add metadata to tell Symposium how to run your mod:

```toml
[package]
name = "my-mod"
version = "0.1.0"
description = "Help agents work with MyLibrary"

[package.metadata.symposium]
# Optional: specify which binary if your crate has multiple
binary = "my-mod"

# Optional: arguments to pass when spawning
args = []

# Optional: environment variables
env = { MY_CONFIG = "value" }
```

The `name`, `description`, and `version` come from the standard `[package]` section.

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
