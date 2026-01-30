# Agent Mods

Agent mods are proxy components that enrich an agent's capabilities. They sit between the editor and the agent, adding tools, context, and behaviors.

## Built-in Mods

| ID | Name | Description |
|----|------|-------------|
| `sparkle` | Sparkle | AI collaboration identity and embodiment |
| `ferris` | Ferris | Rust development tools (crate sources, rust researcher) |
| `cargo` | Cargo | Cargo build and run tools |

## Mod Sources

Mods can come from multiple sources:

- **built-in**: Bundled with Symposium (sparkle, ferris, cargo)
- **registry**: Installed from the [shared agent registry](https://github.com/agentclientprotocol/registry)
- **custom**: User-defined via executable, npx, pipx, cargo, or URL

## Distribution Types

Mods use the same distribution types as agents (see [Agent Registry](./agent-registry.md)):

- `local` - executable command on the system
- `npx` - npm package
- `pipx` - Python package
- `cargo` - Rust crate from crates.io
- `binary` - platform-specific archive download

## Configuration

Mods are passed to `symposium-acp-agent` via `--proxy` arguments:

```bash
symposium-acp-agent run-with --proxy sparkle --proxy ferris --proxy cargo --agent '...'
```

**Order matters** - mods are applied in the order listed. The first mod is closest to the editor, and the last is closest to the agent.

The special value `defaults` expands to all known built-in mods:

```bash
--proxy defaults  # equivalent to: --proxy sparkle --proxy ferris --proxy cargo
```

## Registry Format

The shared registry includes both agents and mods:

```json
{
  "date": "2026-01-07",
  "agents": [...],
  "mods": [
    {
      "id": "some-mod",
      "name": "Some Mod",
      "version": "1.0.0",
      "description": "Does something useful",
      "distribution": {
        "npx": { "package": "@example/some-mod" }
      }
    }
  ]
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Editor Extension (VSCode, Zed, etc.)           │
│  - Manages mod configuration                    │
│  - Builds --proxy args for agent spawn          │
└─────────────────┬───────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────┐
│  symposium-acp-agent                            │
│  - Parses --proxy arguments                     │
│  - Resolves mod distributions                   │
│  - Builds proxy chain in order                  │
│  - Conductor orchestrates the chain             │
└─────────────────────────────────────────────────┘
```

## Mod Discovery and Recommendations

Symposium can suggest mods based on a project's dependencies. This creates a contextual experience where users see relevant mods for their specific codebase.

### Mod Source Naming

Mods are identified using a `source` field with multiple options:

```toml
source.cargo = "foo"           # Rust crate on crates.io
source.registry = "bar"        # Mod ID in ACP registry
source.url = "https://..."     # Direct URL to mod.jsonc
```

### Crate-Defined Recommendations

A crate can recommend mods to its consumers via Cargo.toml metadata:

```toml
[package.metadata.symposium]
# Shorthand for crates.io mods
recommended = ["foo", "bar"]

# Or explicit with full source specification
[[package.metadata.symposium.recommended]]
source.registry = "some-mod"
```

When Symposium detects this crate in a user's dependencies, it surfaces these recommendations.

### External Recommendations

Symposium downloads recommendations from a remote URL at startup:

```
http://recommendations.symposium.dev/recommendations.toml
```

This file maps crates to suggested mods, allowing recommendations without requiring upstream crate changes:

```toml
[[recommendation]]
source.cargo = "tokio-helper"
when.using-crate = "tokio"

[[recommendation]]
source.cargo = "sqlx-helper"
when.using-crates = ["sqlx", "sea-orm"]
```

**Caching behavior:**
- On successful download: cache the file locally
- On download failure: use the cached version if available
- If no cache and download fails: refuse to start (prevents running with no recommendations)

Cache location: `<config_dir>/cache/recommendations.toml`

### User Local Recommendations

Users can add their own recommendation file for custom mappings (e.g., internal/proprietary mods):

Location: `<config_dir>/config/recommendations.toml`

Platform-specific config directories:
- Linux: `~/.config/symposium/`
- macOS: `~/Library/Application Support/symposium/`
- Windows: `%APPDATA%\symposium\`

Local recommendations are merged with remote recommendations.

### Mod Crate Metadata

When a crate *is* a mod (not just recommending one), it declares runtime metadata:

```toml
[package.metadata.symposium]
binary = "my-mod-bin"              # Optional: if crate has multiple binaries
args = ["--mcp", "--some-flag"]    # Optional: arguments to pass
env = { KEY = "value" }            # Optional: environment variables
```

Standard package fields (`name`, `description`, `version`) come from `[package]`. This metadata is used both at runtime and by the GitHub Action that publishes to the ACP registry.

### Discovery Flow

1. Symposium fetches the ACP registry (available mods and their distributions)
2. Symposium loads the recommendations file (external mappings)
3. Symposium scans the user's Cargo.lock for dependencies
4. For each dependency, check:
   - Does the recommendations file have an entry with matching `when.using-crate(s)`?
   - Does the dependency's Cargo.toml have `[package.metadata.symposium.recommended]`?
5. Surface matching mods in the UI as suggestions

### Data Sources

| Source | Purpose | Controlled By |
|--------|---------|---------------|
| ACP Registry | Mod catalog + distribution info | Community |
| Symposium recommendations | External crate-to-mod mappings | Symposium maintainers |
| User recommendation files | Custom mappings | User |
| Cargo.toml metadata | Crate author recommendations | Crate authors |

## Future Work

- **Per-mod configuration**: Add sub-options for mods (e.g., which Ferris tools to enable)
- **Mod updates**: Check for and apply updates to registry-sourced mods
