# `cargo agents init`

Sets up the user-wide configuration.

## Flow

1. **Prompt for agents** — ask which agents the user uses (e.g., Claude Code, Copilot, Gemini). Multiple agents can be selected.

2. **Write user config** — create `~/.symposium/config.toml` with the `[[agent]]` entries populated:

   ```toml
   [[agent]]
   name = "claude"

   [[agent]]
   name = "gemini"
   ```

3. **Register hooks** — register global hooks and MCP servers for each selected agent. Also unregisters hooks for any agents that were removed.

If `--add-agent` or `--remove-agent` flags are provided, the interactive prompt is skipped and the specified changes are applied to the existing agent list.
