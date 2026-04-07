# `cargo agents init --user`

Sets up the user-wide configuration.

## Flow

1. **Prompt for agent** — ask which agent the user uses (e.g., Claude Code, Cursor).

2. **Write user config** — create `~/.cargo-agents/config.toml` with the `[agent]` section populated:

   ```toml
   [agent]
   name = "claude-code"
   sync-default = true
   ```

3. **Run `sync --agent`** — delegates to the [`sync --agent` flow](./sync-agent-flow.md) to register global hooks for the chosen agent. Since there's no project context, this just ensures the global hook is in place (e.g., a global hook in `~/.claude/settings.json` that calls `cargo agents hook` on session start).

> **Combined init**: When `init` runs both user and project setup (either by default or with both flags), user setup runs first, then project setup. The project setup's `sync --agent` step will see the freshly written user config, so hooks end up in the right place.
