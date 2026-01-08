# act-as-configured Mode

## Overview

The `act-as-configured` subcommand reads agent configuration from `~/.symposium/config.jsonc`, simplifying editor setup to a single agent that adapts based on user configuration.

## Config File Format

**Location:** `~/.symposium/config.jsonc`

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

## Behavior

1. **Config exists:** Parse it and run with those settings (equivalent to `act-as-agent --proxy <proxies> -- <agent>`)
2. **No config:** Run a configuration agent that walks the user through setup interactively

## Configuration Agent

When no config file exists, a simple state-machine agent guides users through setup:

1. Presents numbered agent options (Claude Code, Gemini, Codex, Kiro CLI)
2. User types a number to select
3. Config is saved with all proxies enabled by default
4. User is told to restart their editor

The agent expects numeric input (1-N) and repeats the prompt for invalid input.

## Usage

```bash
# Run with configuration
symposium-acp-agent act-as-configured

# With logging
symposium-acp-agent act-as-configured --log debug
```

## Implementation

- Config parsing: `src/symposium-acp-agent/src/config.rs`
- CLI subcommand: `src/symposium-acp-agent/src/main.rs`
- Dependencies: `serde_jsonc`, `shell-words`, `dirs`
