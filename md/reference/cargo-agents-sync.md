# `cargo agents sync`

Synchronize skills with workspace dependencies.

## Usage

```bash
cargo agents sync
```

With the global `-v` flag, sync additionally shows each plugin, skill group, and skill that was evaluated and why each was included or skipped. With `--json`, stdout receives a JSON array of structured event objects (see [global options](./cargo-agents.md#global-options)).

## Behavior

Must be run from within a Rust workspace. Performs the following steps:

1. **Find workspace root**: runs `cargo metadata` to locate the workspace.

2. **Scan dependencies**: reads the full dependency graph from the workspace.

3. **Discover applicable skills**: loads plugin sources (from user config) and matches skill predicates against workspace dependencies. A `SKILL.md` that fails to load, such as one whose frontmatter is invalid or whose `name` is not a plain directory name, is skipped with a warning naming the file.

4. **Install skills**: for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Antigravity/Copilot/Codex). A `.gitignore` containing `*` is written into every new skill directory, and an empty `.symposium` marker file is dropped into each installed skill directory.

5. **Mirror workspace skills**: if `agents-syncing` is enabled (default), user-authored skills in `<workspace>/.agents/skills/` are propagated into the skill directories of any configured agent that doesn't natively use `.agents/skills/` (e.g., `.claude/skills/`, `.kiro/skills/`). See [Workspace skills](../workspace-skills.md).

6. **Clean up stale skills**: scans every agent's skills parent directory and removes any subdirectory containing the `.symposium` marker that wasn't installed (or propagated) this sync. Directories without the marker (user-managed) are left untouched. This step runs even when no agent is configured, so it also removes what was installed for agents since removed. If a registry cannot be read (a `path` that is missing, unreadable or itself a plugin, or a `git` registry that has not been fetched yet), sync warns, naming the registry, and skips this step: nothing is removed until it can be read again.

7. **Register hooks**: ensures hooks and MCP servers are registered for all configured agents, at the scope set by `hook-scope`: the user's home directory for `"global"` (the default), the project for `"project"` (MCP servers fall back to the user level for agents without a project-level file). Unregisters hooks for agents no longer in the config. An agent config file symposium cannot parse is left untouched: sync names it in a warning and carries on with the skills and the other agents, while `cargo agents init` stops with that error.

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

By default (`auto-sync = true`), `cargo agents sync` runs automatically during hook invocations. This keeps skills in sync with workspace dependencies without manual intervention. Set `auto-sync = false` in the user config to disable this and sync manually.

## Example

```bash
# One-time setup
cargo agents init --add-agent claude

# Sync skills for the current workspace
cargo agents sync
```
