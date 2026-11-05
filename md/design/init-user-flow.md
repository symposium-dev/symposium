# `cargo agents init`

Sets up the user-wide configuration.

## Flow

1. **Prompt for agents**: ask which agents the user uses (e.g., Antigravity CLI, Claude Code, Copilot). Multiple agents can be selected. `Agent::detect_installed` looks for each agent's user config directory, and `state.toml` remembers what the previous run detected. The prompt retains explicit choices regardless of detection, and preselects detected agents that are configured or newly detected. An initially unchecked box selected by the user becomes explicit; accepting an automatic default does not. Existing configs migrate as explicit when the state has no `explicit-agents` field. A first run reports them as `Detected: ...` (or `No agents detected` with an empty list); later runs report what changed (`Detected since last setup: ...`, `No longer detected: ...`). Deselecting every agent of an existing config uninstalls; on a first run the prompt insists on at least one. Without a terminal, a first run takes every detected agent and fails asking for `--add-agent` when none is found, while later runs keep the configured agents. The detected set is recorded whenever the user confirms the prompt and on a first run without a terminal, while flag-driven runs update explicit choices and remember removals of currently detected agents without consuming unrelated new detections.

2. **Write user config** — create `~/.symposium/config.toml` with the `[[agent]]` entries populated:

   ```toml
   [[agent]]
   name = "claude"

   [[agent]]
   name = "antigravity"
   ```

3. **Register hooks**: register global hooks and MCP servers for each selected agent. Also unregisters hooks for any agents that were removed (only symposium's handler: a matcher group goes only once that leaves it empty), and removes the MCP servers symposium registered for them in their user-level file (see the [MCP ledger](./state.md#mcp-server-ledger)).

4. **Reap removed agents' skills** - when an agent was removed and `init` runs inside a Rust workspace, run `sync` there, whose stale-skill cleanup takes out that agent's marked copies. Symposium keeps no list of the workspaces it installed into, so copies elsewhere stay until `sync` runs in each one; `sync` with no agents configured still cleans up for this reason, but a config that cannot be read leaves the agents unknown rather than none, so `sync` refuses to run on it.

If `--add-agent` or `--remove-agent` flags are provided, the interactive prompt is skipped and the specified changes are applied to the existing agent list.

An `[[agent]]` name this version does not know is kept as is until `--remove-agent` removes it, and hook registration skips it with a warning, as `sync` does. Only `--add-agent` names and `--remove-agent` names that are not configured must be known agents.
