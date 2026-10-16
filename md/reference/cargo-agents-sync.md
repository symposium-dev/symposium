# `cargo agents sync`

Synchronize skills with workspace dependencies.

## Usage

```bash
cargo agents sync
```

With the global `-v` flag, sync additionally shows each plugin, skill group, and skill that was evaluated and why each was included or skipped. With `--json`, stdout receives a JSON array of structured event objects (see [global options](./cargo-agents.md#global-options)).

## Behavior

Must be run from within a Rust workspace. Performs the following steps:

1. **Find workspace root** — runs `cargo metadata` to locate the workspace.

2. **Scan dependencies** — reads the full dependency graph from the workspace.

3. **Discover applicable skills** — loads plugin sources (from user config) and matches skill predicates against workspace dependencies.

4. **Install skills**: for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Antigravity/Copilot/Codex). A `.gitignore` containing `*` and an empty `.symposium` marker file are written into each installed skill directory; parent directories such as `.claude/skills/` get neither.

5. **Mirror workspace skills** — if `agents-syncing` is enabled (default), user-authored skills in `<workspace>/.agents/skills/` are propagated into the skill directories of any configured agent that doesn't natively use `.agents/skills/` (e.g., `.claude/skills/`, `.kiro/skills/`). See [Workspace skills](../workspace-skills.md).

6. **Clean up stale skills**: scans every agent's skills parent directory and removes any subdirectory containing the `.symposium` marker that wasn't installed (or propagated) this sync. Directories without the marker (user-managed) are left untouched. If a registry could not be read in full (a `path` that is missing or unreadable, or a `git` registry whose local copy is missing or incomplete), sync warns, naming the registry, and skips this step: nothing is removed until every registry can be read again, while skills from the registries that did load are still installed and updated.

7. **Register hooks** — ensures hooks and MCP servers are registered for all configured agents. Registers both global hooks (for all projects) and project-specific hooks (for the current project). Unregisters hooks for agents no longer in the config.

## Consent prompt

Before syncing, an interactive `cargo agents sync` asks about each dependency
whose source embeds an agent plugin that you have not decided about yet.
Depending on a crate means compiling its code, not letting its author inject
agent context, so these stay off until you say otherwise. Three answers:

- **Ask me later** (the default) — records nothing; you are asked again next time.
- **Enable** — recorded in `[plugins] auto-enable`, and installed by this same sync.
- **No — don't ask again** — recorded in `[plugins] disable`.

Only explicit answers are recorded, so hitting Enter through the prompt never
permanently declines anything. Escape leaves the remaining questions undecided.

The prompt only runs in a real terminal session. The automatic sync below —
and anything else an agent triggers — never prompts. Instead, your agent is told
so it can point you here: pending candidates are named when a session starts,
and when your dependencies change mid-session the agent hears that there may be
new ones. [`cargo agents status`](./cargo-agents-status.md) lists them as
`candidate`.

## Automatic sync

By default (`auto-sync = true`), `cargo agents sync` runs automatically during hook invocations. This keeps skills in sync with workspace dependencies without manual intervention. Set `auto-sync = false` in the user config to disable this and sync manually. The automatic sync skips cleanup for an unreadable registry the same way; its warnings go to the log file instead of your terminal.

## Example

```bash
# One-time setup
cargo agents init --add-agent claude

# Sync skills for the current workspace
cargo agents sync
```
