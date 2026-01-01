# Zed, RustRover, Neovim, and other editors

For ACP-compatible editors, install the Symposium agent binary:

```bash
cargo binstall symposium-acp-agent
```

Then configure your editor to use `symposium-acp-agent` as the agent command, passing your preferred downstream agent. For example, with Claude Code:

```bash
symposium-acp-agent -- claude-code --acp
```

Or with Zed's Claude integration:

```bash
symposium-acp-agent -- npx -y @anthropic-ai/claude-code-zed
```

The `--` separates Symposium's arguments from the downstream agent command.
