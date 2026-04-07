# `cargo agents hook`

Entry point invoked by the agent's hook system on session events.

## Flow

1. **Run `sync --agent`** — delegates to the [`sync --agent` flow](./sync-agent-flow.md) to ensure extensions are installed and hooks are current.

2. **Dispatch to plugin hooks** — for each enabled plugin that defines a hook handler for the incoming event:
   - Pass the event JSON on stdin to the plugin's hook command.
   - Collect output from each handler.
   - Merge results (e.g., allow/block decisions, output text) across all handlers.
   - Return the merged result to the agent.

Plugin hooks can respond to agent-specific events (e.g., `pre-tool-use`, `post-tool-use`, `user-prompt-submit` for Claude Code). The available events depend on which agent is in use.
