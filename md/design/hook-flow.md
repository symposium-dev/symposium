# `symposium hook`

Entry point invoked by the agent's hook system on session events.

## Flow

1. **Auto-sync** (if enabled) — when `auto-sync = true` in the user config, runs [`symposium sync`](./sync-agent-flow.md) to ensure skills are current. The workspace root is resolved from the payload's `cwd` field; if the payload does not include a working directory, the process's current working directory is used as a fallback. Runs quietly and non-fatally — failures are logged but don't block hook dispatch.

2. **Dispatch to plugin hooks** — for each enabled plugin that defines a hook handler for the incoming event:
   - Pass the event JSON on stdin to the plugin's hook command.
   - Collect output from each handler.
   - Merge results (e.g., allow/block decisions, output text) across all handlers.
   - Return the merged result to the agent.

Plugin hooks can respond to agent-specific events (e.g., `pre-tool-use`, `post-tool-use`, `user-prompt-submit` for Claude Code). The available events depend on which agent is in use.
