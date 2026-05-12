# `cargo agents sync`

Synchronize skills with workspace dependencies.

## Usage

```bash
cargo agents sync
```

## Behavior

Must be run from within a Rust workspace. Performs the following steps:

1. **Find workspace root** — runs `cargo metadata` to locate the workspace.

2. **Scan dependencies** — reads the full dependency graph from the workspace.

3. **Discover applicable skills** — loads plugin sources (from user config) and matches skill predicates against workspace dependencies.

4. **Install skills** — for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Copilot/Gemini/Codex). A `.gitignore` containing `*` is written into every new skill directory (and its `skills/` parent if new), and an empty `.symposium` marker file is dropped into each installed skill directory.

5. **Mirror workspace skills** — if `agents-syncing` is enabled (default), user-authored skills in `<workspace>/.agents/skills/` are propagated into the skill directories of any configured agent that doesn't natively use `.agents/skills/` (e.g., `.claude/skills/`, `.kiro/skills/`). See [Workspace skills](../workspace-skills.md).

6. **Clean up stale skills** — scans every agent's skills parent directory and removes any subdirectory containing the `.symposium` marker that wasn't installed (or propagated) this sync. Directories without the marker (user-managed) are left untouched.

7. **Register hooks** — ensures hooks and MCP servers are registered for all configured agents. Registers both global hooks (for all projects) and project-specific hooks (for the current project). Unregisters hooks for agents no longer in the config.

## Automatic sync

By default (`auto-sync = true`), `cargo agents sync` runs automatically during hook invocations. This keeps skills in sync with workspace dependencies without manual intervention. Set `auto-sync = false` in the user config to disable this and sync manually.

## Example

```bash
# One-time setup
cargo agents init --add-agent claude

# Sync skills for the current workspace
cargo agents sync
```
