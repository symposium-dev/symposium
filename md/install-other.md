# Other Editors

Symposium works with any editor that supports ACP. See the [editors on ACP](https://zed.dev/acp#editors-on-acp) page for a list of supported editors and how to install ACP support.

## Installation

1. Install ACP support in your editor of choice
2. Install the Symposium agent binary:
   ```bash
   cargo binstall symposium-acp-agent
   ```
   or from source:
   ```bash
   cargo install symposium-acp-agent
   ```
3. Configure your editor to run:
   ```
   ~/.cargo/bin/symposium-acp-agent run
   ```

Instructions for configuring ACP support in common editors can be found here:

- [RustRover, IntelliJ, and other JetBrains IDEs](https://www.jetbrains.com/help/ai-assistant/acp.html)
- [NeoVim (CodeCompanion)](https://codecompanion.olimorris.dev/configuration/adapters-acp)
- [Emacs (agent-shell)](https://github.com/xenodium/agent-shell?tab=readme-ov-file#configuration)

## Configuring Symposium

On first run, Symposium will ask you a few questions to create your configuration file at `~/.symposium/config.jsonc`:

```
Welcome to Symposium!

No configuration found. Let's set up your AI agent.

Which agent would you like to use?

  1. Claude Code
  2. Gemini CLI
  3. Codex
  4. Kiro CLI

Type a number (1-4) to select:
```

After selecting an agent, Symposium creates the config file and you can restart your editor to start using it.

### Manual Configuration

You can edit `~/.symposium/config.jsonc` directly for more control. The format is:

```jsonc
{
  "agent": "npx -y @zed-industries/claude-code-acp",
  "proxies": [
    { "name": "sparkle", "enabled": true },
    { "name": "ferris", "enabled": true },
    { "name": "cargo", "enabled": true }
  ]
}
```

**Fields:**

- **`agent`**: The command to run your downstream AI agent. This is passed to the shell, so you can use any command that works in your terminal.

- **`proxies`**: List of Symposium mods to enable. Each entry has:
  - `name`: The mod name
  - `enabled`: Set to `true` or `false` to enable/disable

### Built-in Mods

| Name | Description |
|------|-------------|
| `sparkle` | AI collaboration identity and memory |
| `ferris` | Rust crate source fetching |
| `cargo` | Cargo build/test/check commands |
