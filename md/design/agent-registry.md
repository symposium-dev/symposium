# Agent Registry

Symposium supports multiple ACP-compatible agents and extensions. Users can select from built-in defaults or add entries from the [ACP Agent Registry](https://github.com/agentclientprotocol/registry).

The registry resolution logic lives in `symposium-acp-agent` and is shared across all editor integrations.

## Agent Configuration

Each agent or extension is represented as an `AgentConfig` object:

```typescript
interface AgentConfig {
  // Required fields
  id: string;
  distribution: {
    local?: { command: string; args?: string[]; env?: Record<string, string> };
    symposium?: { subcommand: string; args?: string[] };
    npx?: { package: string; args?: string[] };
    pipx?: { package: string; args?: string[] };
    cargo?: { crate: string; version?: string; binary?: string; args?: string[] };
    binary?: {
      [platform: string]: {    // e.g., "darwin-aarch64", "linux-x86_64"
        archive: string;
        cmd: string;
        args?: string[];
      };
    };
  };

  // Optional fields (populated from registry if imported)
  name?: string;               // display name, defaults to id
  version?: string;
  description?: string;
  // ... other registry fields as needed

  // Source tracking
  _source?: "registry" | "custom";  // defaults to "custom" if omitted
}
```

### Built-in Agents

Three agents ship as defaults with `_source: "custom"`:

```json
[
  {
    "id": "zed-claude-code",
    "name": "Claude Code",
    "distribution": { "npx": { "package": "@zed-industries/claude-code-acp@latest" } }
  },
  {
    "id": "elizacp",
    "name": "ElizACP",
    "description": "Built-in Eliza agent for testing",
    "distribution": { "symposium": { "subcommand": "eliza" } }
  },
  {
    "id": "kiro-cli",
    "name": "Kiro CLI",
    "distribution": { "local": { "command": "kiro-cli-chat", "args": ["acp"] } }
  }
]
```

### Registry-Imported Agents

When a user imports an agent from the registry, the full registry entry is stored with `_source: "registry"`:

```json
{
  "id": "gemini",
  "name": "Gemini CLI",
  "version": "0.22.3",
  "description": "Google's official CLI for Gemini",
  "_source": "registry",
  "distribution": {
    "npx": { "package": "@google/gemini-cli@0.22.3", "args": ["--experimental-acp"] }
  }
}
```

### Custom Agents

Users can manually add agents with minimal configuration:

```json
{
  "id": "my-agent",
  "distribution": { "npx": { "package": "my-agent-package" } }
}
```

## Registry Sync

For agents with `_source: "registry"`, the extension checks for updates and applies them automatically. Agents removed from the registry are left unchanged—the configuration still works, it just won't receive future updates.

The registry URL:
```
https://github.com/agentclientprotocol/registry/releases/latest/download/registry.json
```

## Spawning an Agent

At spawn time, the extension resolves the distribution to a command (priority order):

1. If `distribution.local` exists → `{command} {args...}` with optional env vars
2. Else if `distribution.symposium` exists → run as symposium subcommand
3. Else if `distribution.npx` exists → `npx -y {package} {args...}`
4. Else if `distribution.pipx` exists → `pipx run {package} {args...}`
5. Else if `distribution.cargo` exists → install and run Rust crate (see below)
6. Else if `distribution.binary[currentPlatform]` exists:
   - Check `~/.symposium/bin/{id}/{version}/` for cached binary
   - If not present, download and extract from `archive`
   - Execute `{cache-path}/{cmd} {args...}`
7. Else → error (no compatible distribution for this platform)

### Cargo Distribution

The cargo distribution installs agents/extensions from crates.io:

```json
{
  "id": "my-rust-extension",
  "distribution": {
    "cargo": {
      "crate": "my-acp-extension",
      "version": "0.1.0"
    }
  }
}
```

Resolution process:

1. **Version resolution**: If no version specified, query crates.io for the latest stable version
2. **Binary discovery**: Query crates.io API for the crate's `bin_names` field to determine the executable name
3. **Cache check**: Look for `~/.symposium/bin/{id}/{version}/bin/{binary}`
4. **Installation**: If not cached:
   - Try `cargo binstall --no-confirm --root {cache-dir} {crate}@{version}` (uses prebuilt binaries, fast)
   - If binstall fails or unavailable, fall back to `cargo install --root {cache-dir} {crate}@{version}` (builds from source)
5. **Cleanup**: Delete old versions when installing a new one

The `binary` field is optional—if omitted, it's discovered from crates.io. If the crate has multiple binaries, the field is required to disambiguate.

### Platform Detection

Map from Node.js to registry platform keys:

| `process.platform` | `process.arch` | Registry Key |
|--------------------|----------------|--------------|
| `darwin` | `arm64` | `darwin-aarch64` |
| `darwin` | `x64` | `darwin-x86_64` |
| `linux` | `x64` | `linux-x86_64` |
| `linux` | `arm64` | `linux-aarch64` |
| `win32` | `x64` | `windows-x86_64` |

## CLI Commands

The `symposium-acp-agent` binary provides registry subcommands:

```bash
# List all available agents (built-ins + registry)
symposium-acp-agent registry list

# Resolve an agent ID to an executable command (McpServer JSON)
symposium-acp-agent registry resolve <agent-id>
```

The `registry list` output is a JSON array of `{id, name, version?, description?}` objects.

The `registry resolve` output is an `McpServer` JSON object ready for spawning:
```json
{"name":"Agent Name","command":"/path/to/binary","args":["--flag"],"env":[]}
```

## Decisions

- **Binary cleanup**: Delete old versions when downloading a new one. No accumulation.
- **Registry caching**: Registry is cached in memory during a session and fetched fresh on first access.
