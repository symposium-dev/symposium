# `symposium sync`

Synchronize skills with workspace dependencies.

## Usage

```bash
symposium sync
```

## Behavior

Must be run from within a Rust workspace. Performs the following steps:

1. **Find workspace root** — runs `cargo metadata` to locate the workspace.

2. **Scan dependencies** — reads the full dependency graph from the workspace.

3. **Discover applicable skills** — loads plugin sources (from user config) and matches skill predicates against workspace dependencies.

4. **Install skills** — for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Copilot/Gemini/Codex).

5. **Clean up stale skills** — removes skills that were previously installed by symposium but are no longer applicable (e.g., because a dependency was removed). Tracks installed skills in a per-agent manifest (`.symposium.toml` in the agent's skill directory). Skills not in the manifest (user-managed) are left untouched.

6. **Register hooks** — ensures global hooks and MCP servers are registered for all configured agents. Unregisters hooks for agents no longer in the config.

## Automatic sync

By default (`auto-sync = true`), `symposium sync` runs automatically during hook invocations. This keeps skills in sync with workspace dependencies without manual intervention. Set `auto-sync = false` in the user config to disable this and sync manually.

## Example

```bash
# One-time setup
symposium init --add-agent claude

# Sync skills for the current workspace
symposium sync
```
