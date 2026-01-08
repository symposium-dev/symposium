# Agent Registry Integration

The VSCode extension supports multiple ACP-compatible agents. Users can select from built-in defaults or add agents from the [ACP Agent Registry](https://github.com/agentclientprotocol/registry).

## Agent Configuration

Agent configurations are stored in VSCode settings. Each agent is represented as an `AgentConfig` object:

```typescript
interface AgentConfig {
  // Required fields
  id: string;
  distribution: {
    local?: { command: string; args?: string[]; env?: Record<string, string> };
    symposium?: { subcommand: string; args?: string[] };
    npx?: { package: string; args?: string[] };
    pipx?: { package: string; args?: string[] };
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
5. Else if `distribution.binary[currentPlatform]` exists:
   - Check `~/.symposium/bin/{id}/{version}/` for cached binary
   - If not present, download and extract from `archive`
   - Execute `{cache-path}/{cmd} {args...}`
6. Else → error (no compatible distribution for this platform)

### Platform Detection

Map from Node.js to registry platform keys:

| `process.platform` | `process.arch` | Registry Key |
|--------------------|----------------|--------------|
| `darwin` | `arm64` | `darwin-aarch64` |
| `darwin` | `x64` | `darwin-x86_64` |
| `linux` | `x64` | `linux-x86_64` |
| `linux` | `arm64` | `linux-aarch64` |
| `win32` | `x64` | `windows-x86_64` |

## User Interface

### Agent Selection

The extension provides UI for:
- Viewing configured agents
- Selecting the active agent
- Adding agents from registry (opens picker dialog)
- Removing agents (built-ins can be removed but will reappear on reset)

### Add from Registry Flow

Uses VSCode's QuickPick dialog to show agents not already configured. The registry is fetched on-demand when the user triggers the action. Each item shows the agent name, version, and description.

## Decisions

- **Binary cleanup**: Delete old versions when downloading a new one. No accumulation.
- **Registry caching**: Registry is cached in memory during a session and fetched fresh on first access.
