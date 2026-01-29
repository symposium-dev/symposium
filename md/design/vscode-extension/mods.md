# Mods UI (VSCode)

This chapter covers how mods work in the VSCode environment. For general mod concepts, see [Agent Mods](../mods.md).

## Configuration

Mods are configured through the Symposium config agent, not through VSCode settings. The VSCode extension spawns `symposium-acp-agent run`, which handles all configuration.

See [Run Mode](../run-mode.md) for details on:
- Configuration file format (`~/.symposium/config/`)
- Interactive configuration via `/symposium:config` command
- First-time setup flow

## Spawn Integration

The VSCode extension spawns the agent using `symposium-acp-agent run`. The config agent loads mod configuration from disk and builds the proxy chain automatically.
