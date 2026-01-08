# Act-as-Configured Mode

The `act-as-configured` subcommand simplifies editor integration by reading agent configuration from a file rather than requiring command-line arguments.

## Motivation

Without this mode, editor extensions must either:
- Hardcode specific agent commands, requiring extension updates to add new agents
- Expose complex configuration UI for specifying agent commands and proxy options

With `act-as-configured`, the extension simply runs:

```bash
symposium-acp-agent act-as-configured
```

The agent reads its configuration from `~/.symposium/config.jsonc`, and if no configuration exists, runs an interactive setup wizard.

## Configuration File

**Location:** `~/.symposium/config.jsonc`

The file uses JSONC (JSON with comments) format:

```jsonc
{
  // Downstream agent command (parsed as shell words)
  "agent": "npx -y @zed-industries/claude-code-acp",
  
  // Proxy extensions to enable
  "proxies": [
    { "name": "sparkle", "enabled": true },
    { "name": "ferris", "enabled": true },
    { "name": "cargo", "enabled": true }
  ]
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `agent` | string | Shell command to spawn the downstream agent. Parsed using shell word splitting. |
| `proxies` | array | List of proxy extensions with `name` and `enabled` fields. |

The `agent` string is parsed as shell words, so commands like `npx -y @zed-industries/claude-code-acp` work correctly.

## Runtime Behavior

```
┌─────────────────────────────────────────┐
│         act-as-configured               │
└─────────────────┬───────────────────────┘
                  │
                  ▼
        ┌─────────────────┐
        │ Config exists?  │
        └────────┬────────┘
                 │
         ┌───────┴───────┐
         │               │
         ▼               ▼
   ┌──────────┐   ┌──────────────┐
   │  Yes     │   │     No       │
   └────┬─────┘   └──────┬───────┘
        │                │
        ▼                ▼
   Load config     Run configuration
   Run agent       agent (setup wizard)
```

When a configuration file exists, `act-as-configured` behaves equivalently to:

```bash
symposium-acp-agent act-as-agent \
    --proxy sparkle --proxy ferris --proxy cargo \
    -- npx -y @zed-industries/claude-code-acp
```

## Configuration Agent

When no configuration file exists, Symposium runs a built-in configuration agent instead of a downstream AI agent. This agent:

1. Presents a numbered list of known agents (Claude Code, Gemini, Codex, Kiro CLI)
2. Waits for the user to type a number (1-N)
3. Saves the configuration file with all proxies enabled
4. Instructs the user to restart their editor

The configuration agent is a simple state machine that expects numeric input. Invalid input causes the prompt to repeat.

### Known Agents

The configuration wizard offers these pre-configured agents:

| Name | Command |
|------|---------|
| Claude Code | `npx -y @zed-industries/claude-code-acp` |
| Gemini CLI | `npx -y -- @google/gemini-cli@latest --experimental-acp` |
| Codex | `npx -y @zed-industries/codex-acp` |
| Kiro CLI | `kiro-cli-chat acp` |

Users can manually edit `~/.symposium/config.jsonc` to use other agents or modify proxy settings.

## Implementation

The implementation consists of:

- **Config types:** `SymposiumUserConfig` and `ProxyEntry` structs in `src/symposium-acp-agent/src/config.rs`
- **Config loading:** `load()` reads from `~/.symposium/config.jsonc`, `save()` writes it
- **Configuration agent:** `ConfigurationAgent` implements the ACP `Component` trait
- **CLI integration:** `ActAsConfigured` variant in the `Command` enum

### Dependencies

| Crate | Purpose |
|-------|---------|
| `serde_jsonc` | Parse JSON with comments |
| `shell-words` | Parse agent command string into arguments |
| `dirs` | Cross-platform home directory resolution |
