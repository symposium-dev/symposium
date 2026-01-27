# Agent Extensions

Agent extensions are proxy components that enrich an agent's capabilities. They sit between the editor and the agent, adding tools, context, and behaviors.

## Built-in Extensions

| ID | Name | Description |
|----|------|-------------|
| `sparkle` | Sparkle | AI collaboration identity and embodiment |
| `ferris` | Ferris | Rust development tools (crate sources, rust researcher) |
| `cargo` | Cargo | Cargo build and run tools |

## Extension Sources

Extensions can come from multiple sources:

- **built-in**: Bundled with Symposium (sparkle, ferris, cargo)
- **registry**: Installed from the [shared agent registry](https://github.com/agentclientprotocol/registry)
- **custom**: User-defined via executable, npx, pipx, cargo, or URL

## Distribution Types

Extensions use the same distribution types as agents (see [Agent Registry](./agent-registry.md)):

- `local` - executable command on the system
- `npx` - npm package
- `pipx` - Python package  
- `cargo` - Rust crate from crates.io
- `binary` - platform-specific archive download

## Configuration

Extensions are passed to `symposium-acp-agent` via `--proxy` arguments:

```bash
symposium-acp-agent run-with --proxy sparkle --proxy ferris --proxy cargo --agent '...'
```

**Order matters** - extensions are applied in the order listed. The first extension is closest to the editor, and the last is closest to the agent.

The special value `defaults` expands to all known built-in extensions:

```bash
--proxy defaults  # equivalent to: --proxy sparkle --proxy ferris --proxy cargo
```

## Registry Format

The shared registry includes both agents and extensions:

```json
{
  "date": "2026-01-07",
  "agents": [...],
  "extensions": [
    {
      "id": "some-extension",
      "name": "Some Extension",
      "version": "1.0.0",
      "description": "Does something useful",
      "distribution": {
        "npx": { "package": "@example/some-extension" }
      }
    }
  ]
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Editor Extension (VSCode, Zed, etc.)           │
│  - Manages extension configuration              │
│  - Builds --proxy args for agent spawn          │
└─────────────────┬───────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────┐
│  symposium-acp-agent                            │
│  - Parses --proxy arguments                     │
│  - Resolves extension distributions             │
│  - Builds proxy chain in order                  │
│  - Conductor orchestrates the chain             │
└─────────────────────────────────────────────────┘
```

## Extension Discovery and Recommendations

Symposium can suggest extensions based on a project's dependencies. This creates a contextual experience where users see relevant extensions for their specific codebase.

### Extension Source Naming

Extensions are identified using a `source` field with multiple options:

```toml
source.cargo = "foo"           # Rust crate on crates.io
source.registry = "bar"        # Extension ID in ACP registry
source.url = "https://..."     # Direct URL to extension.jsonc
```

### Crate-Defined Recommendations

A crate can recommend extensions to its consumers via Cargo.toml metadata:

```toml
[package.metadata.symposium]
# Shorthand for crates.io extensions
recommended = ["foo", "bar"]

# Or explicit with full source specification
[[package.metadata.symposium.recommended]]
source.registry = "some-extension"
```

When Symposium detects this crate in a user's dependencies, it surfaces these recommendations.

### External Recommendations

Symposium maintains a recommendations file that maps crates to suggested extensions. This allows recommendations without requiring upstream crate changes:

```toml
[[recommendation]]
source.cargo = "tokio-helper"
when.using-crate = "tokio"

[[recommendation]]
source.cargo = "sqlx-helper"
when.using-crates = ["sqlx", "sea-orm"]
```

Users can add their own recommendation files for custom mappings.

### Extension Crate Metadata

When a crate *is* an extension (not just recommending one), it declares runtime metadata:

```toml
[package.metadata.symposium]
binary = "my-extension-bin"        # Optional: if crate has multiple binaries
args = ["--mcp", "--some-flag"]    # Optional: arguments to pass
env = { KEY = "value" }            # Optional: environment variables
```

Standard package fields (`name`, `description`, `version`) come from `[package]`. This metadata is used both at runtime and by the GitHub Action that publishes to the ACP registry.

### Discovery Flow

1. Symposium fetches the ACP registry (available extensions and their distributions)
2. Symposium loads the recommendations file (external mappings)
3. Symposium scans the user's Cargo.lock for dependencies
4. For each dependency, check:
   - Does the recommendations file have an entry with matching `when.using-crate(s)`?
   - Does the dependency's Cargo.toml have `[package.metadata.symposium.recommended]`?
5. Surface matching extensions in the UI as suggestions

### Data Sources

| Source | Purpose | Controlled By |
|--------|---------|---------------|
| ACP Registry | Extension catalog + distribution info | Community |
| Symposium recommendations | External crate-to-extension mappings | Symposium maintainers |
| User recommendation files | Custom mappings | User |
| Cargo.toml metadata | Crate author recommendations | Crate authors |

## Future Work

- **Per-extension configuration**: Add sub-options for extensions (e.g., which Ferris tools to enable)
- **Extension updates**: Check for and apply updates to registry-sourced extensions
