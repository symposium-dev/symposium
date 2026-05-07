# `cargo agents hook`

Entry point invoked by the agent's hook system on session events.

## Flow

1. **Auto-sync** (if enabled) — when `auto-sync = true` in the user config, runs [`cargo agents sync`](./sync-agent-flow.md) to ensure skills are current. The workspace root is resolved from the payload's `cwd` field; if the payload does not include a working directory, the process's current working directory is used as a fallback. Runs quietly and non-fatally — failures are logged but don't block hook dispatch.

2. **Dispatch to plugin hooks** — for each enabled plugin that defines a hook handler for the incoming event:
   - Ensure any `requirements` for the hook are acquired (on-demand, best-effort).
   - Resolve the hook's `command` (a named installation reference or inline spec) into a runnable form:
     - `shell` source → spawn `sh -c <command>`.
     - any other source → acquire (install / cache / clone as needed) and obtain a path on disk; for `github`, the validated `sub_path` picks the file inside the cached repo. Spawn the path with the resolved `args`.
   - Pass the event JSON on stdin to the plugin's hook.
   - Collect output from each handler.
   - Merge results (e.g., allow/block decisions, output text) across all handlers.
   - Return the merged result to the agent.

Plugin hooks can respond to agent-specific events (e.g., `pre-tool-use`, `post-tool-use`, `user-prompt-submit` for Claude Code). The available events depend on which agent is in use.
