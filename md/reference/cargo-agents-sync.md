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

4. **Install skills** — for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Antigravity/Copilot/Codex). A `.gitignore` containing `*` is written into every new skill directory (and its `skills/` parent if new), and an empty `.symposium` marker file is dropped into each installed skill directory.

5. **Mirror workspace skills** — if `agents-syncing` is enabled (default), user-authored skills in `<workspace>/.agents/skills/` are propagated into the skill directories of any configured agent that doesn't natively use `.agents/skills/` (e.g., `.claude/skills/`, `.kiro/skills/`). See [Workspace skills](../workspace-skills.md).

6. **Clean up stale skills** — scans every agent's skills parent directory and removes any subdirectory containing the `.symposium` marker that wasn't installed (or propagated) this sync. Directories without the marker (user-managed) are left untouched.

7. **Register hooks** — ensures hooks and MCP servers are registered for all configured agents. Registers both global hooks (for all projects) and project-specific hooks (for the current project). Unregisters hooks for agents no longer in the config.

## Plugins enabled for every workspace

A plugin you enabled with [`cargo agents use --global`](./cargo-agents-use.md) is installed for you, not copied into each project. Sync compiles it into one plugin directory under `~/.symposium/installed/<plugin>/` and gives that directory to every configured agent that can install a plugin for the user, whatever [`hook-scope`](./configuration.md) says:

| Agent | Where the plugin goes |
|-------|-----------------------|
| Antigravity CLI | its skills, per project, as above |
| Claude Code | `~/.claude/skills/<plugin>/` (or `$CLAUDE_CONFIG_DIR/skills/<plugin>/`), loaded as the plugin `<plugin>` in every project |
| Codex CLI | `~/.codex/plugins/cache/symposium/<plugin>/` (or under `$CODEX_HOME`), enabled as `<plugin>@symposium` in `config.toml` and loaded in every project |
| GitHub Copilot | its skills, per project, as above |
| Goose | its skills, per project, as above |
| Kiro | its skills, per project, as above |
| OpenCode | its skills, per project, as above |

An agent that received the plugin gets no per-project copy of its skills, and copies an earlier sync left are cleaned up. Agents load plugins when a session starts, so a plugin installed by the automatic sync at session start appears in the next session (in Claude Code, `/reload-plugins` loads it right away).

The plugin's skills are still selected in the workspace being synced, so a skill that does not apply there is left out until a workspace where it applies syncs. Dropping the `--global` entry, or removing the agent with `cargo agents init --remove-agent`, removes what symposium installed.

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
