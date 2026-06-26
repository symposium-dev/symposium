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

3. **Load used sources** — resolves `[used.crates]`, `used.paths`, and `used.git` entries from the user config. Checks for updates based on source type: crates.io and git check on a throttled cadence (at most once per 24 hours); path sources always check mtime.

4. **Resolve discovery policy** — collects `[discovery.allow]` / `[discovery.deny]` rules from both the user config and all plugin crates in use. Checks workspace deps against the combined policy and fetches any matching crates as additional plugin sources.

5. **Resolve transitive plugin sources** — follows `[[plugins]] source.*` entries from all loaded plugins (recursively, with `where.*` predicate evaluation).

6. **Discover applicable skills** — scans each plugin source crate for `SYMPOSIUM.toml` files (or falls back to `$ROOT/skills/`), then matches skill predicates against workspace dependencies.

7. **Install skills** — for each configured agent, copies applicable `SKILL.md` files into the agent's expected skill directory (e.g., `.claude/skills/` for Claude Code, `.agents/skills/` for Copilot/Gemini/Codex). A `.gitignore` containing `*` is written into every new skill directory (and its `skills/` parent if new), and an empty `.symposium` marker file is dropped into each installed skill directory.

8. **Mirror workspace skills** — if `agents-syncing` is enabled (default), user-authored skills in `<workspace>/.agents/skills/` are propagated into the skill directories of any configured agent that doesn't natively use `.agents/skills/` (e.g., `.claude/skills/`, `.kiro/skills/`). See [Workspace plugins](../workspace.md).

9. **Clean up stale skills** — scans every agent's skills parent directory and removes any subdirectory containing the `.symposium` marker that wasn't installed (or propagated) this sync. Directories without the marker (user-managed) are left untouched.

10. **Register hooks** — ensures hooks and MCP servers are registered for all configured agents. Registers both global hooks (for all projects) and project-specific hooks (for the current project). Unregisters hooks for agents no longer in the config.

## Automatic sync

By default (`auto-sync = true`), `cargo agents sync` runs automatically during hook invocations. This keeps skills in sync with workspace dependencies without manual intervention. Set `auto-sync = false` in the user config to disable this and sync manually.

## Example

```bash
# One-time setup
cargo agents init --add-agent claude

# Sync skills for the current workspace
cargo agents sync
```
