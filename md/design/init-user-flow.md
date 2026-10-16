# `cargo agents init`

Sets up the user-wide configuration.

## Flow

1. **Prompt for agents**: ask which agents the user uses (e.g., Antigravity CLI, Claude Code, Copilot). Multiple agents can be selected. `Agent::detect_installed` looks for each agent's user config directory, and `state.toml` remembers what the previous run detected. The prompt preselects the detected agents that are configured or were not detected before, so an agent the user deselected stays unchecked. A first run reports them as `Detected: ...` (or `No agents detected` with an empty list); later runs report what changed (`Detected since last setup: ...`, `No longer detected: ...`). Deselecting every agent of an existing config uninstalls; on a first run the prompt insists on at least one. Without a terminal, a first run takes every detected agent and fails asking for `--add-agent` when none is found, while later runs keep the configured agents. The detected set is recorded whenever the user confirms the prompt and on a first run without a terminal, never when `--add-agent` / `--remove-agent` drive the run.

2. **Write user config** — create `~/.symposium/config.toml` with the `[[agent]]` entries populated:

   ```toml
   [[agent]]
   name = "claude"

   [[agent]]
   name = "antigravity"
   ```

3. **Register hooks** — register global hooks and MCP servers for each selected agent. Also unregisters hooks for any agents that were removed.

If `--add-agent` or `--remove-agent` flags are provided, the interactive prompt is skipped and the specified changes are applied to the existing agent list.
