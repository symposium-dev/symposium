# `cargo agents sync --agent`

Installs enabled extensions into the agent's expected locations and ensures hooks are registered.

## Flow

### When run inside a project

1. **Determine agent** — check `.symposium/config.toml` for a project-level `[agent]` override. If not set, fall back to the user-wide config.

2. **Ensure hooks are registered** — where the hooks are placed depends on where the agent setting comes from:
   - **Project-level agent**: install hooks into the project's agent config (e.g., `.claude/hooks.json` for Claude Code).
   - **User-level agent**: install hooks into the global agent config (e.g., `~/.claude/settings.json` for Claude Code).

3. **Install extensions** — read `.symposium/config.toml` and, for each enabled extension:
   - **Skills**: resolve the skill source (local or git), copy/symlink `SKILL.md` files into the agent's expected location (e.g., `.claude/skills/` for Claude Code).
   - **Workflows**: install workflow definitions into the appropriate agent location.

### When run outside a project

1. **Read user config** — load `~/.symposium/config.toml` to determine the agent.

2. **Ensure global hooks are registered** — install hooks into the global agent config. This is all that can be done without a project context.
