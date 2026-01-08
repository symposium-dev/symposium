# Symposium Zed Extension

This extension adds the Symposium agent server to Zed, enriching AI agents with Rust-focused tools.

## What is Symposium?

Symposium wraps AI agents with additional capabilities:

- **Sparkle**: Collaborative AI identity and memory
- **Ferris**: Rust crate source inspection and research
- **Cargo**: Cargo build/test/check integration

## First-Time Setup

On first run, Symposium will guide you through configuration:

1. Select your preferred AI agent (Claude Code, Gemini, Codex, or Kiro CLI)
2. Configuration is saved to `~/.symposium/config.jsonc`
3. Restart Zed to use your configured agent

## Requirements

- **Node.js**: Required for Claude Code, Codex, and Gemini (uses npx)
- **API keys**: Each agent requires its respective API key configured

## Installation

Install from the Zed extension marketplace:

1. Open Zed
2. Open the command palette (Cmd+Shift+P / Ctrl+Shift+P)
3. Search for "Extensions: Install Extension"
4. Search for "Symposium"

## Usage

After installation, select "Symposium" in Zed's agent panel. On first use, you'll be guided through selecting your preferred AI agent.

To change your agent later, edit `~/.symposium/config.jsonc` or delete it to run setup again.

## Links

- [Symposium Documentation](https://symposium-dev.github.io/symposium/)
- [GitHub Repository](https://github.com/symposium-dev/symposium)
