# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Load installed crates** — read the user config's `[[installed-crate]]` entries and fetch their sources via `RustCrateFetch` (using the cargo probe technique for crates.io/git sources, or reading path sources directly). Check for updates: crates.io and git sources use a throttled cadence (at most once per 24 hours); path sources always check mtime.

3. **Scan dependencies** — read the full dependency graph from the workspace.

4. **Resolve dependency allow list** — collect `dependency-allow-list` entries from the user config and all loaded plugins. Check workspace deps against the combined list; fetch any matching crates as additional plugin sources.

5. **Resolve auto-installs** — follow `[[auto-install]]` entries from all loaded plugins recursively, evaluating predicates against the workspace. Fetch each resolved crate.

6. **Discover plugins** — for each plugin source crate, walk from the crate root looking for `SYMPOSIUM.toml` files (pruning subtrees when found). If none found, fall back to scanning `$ROOT/skills/` for `SKILL.md` files. Read crate Cargo metadata to populate implicit installations from binary targets.

7. **Match skills to dependencies** — for each plugin, evaluate plugin-level predicates, then evaluate skill group predicates and individual skill `crates` frontmatter against the workspace dependencies.

8. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Drop a `.symposium` marker file into each installed skill directory so future syncs (and other tools) can recognize it as symposium-managed.
   - For every skill directory symposium creates along the way (the skill directory itself or its `skills/` parent), write a `.gitignore` containing a single `*` so symposium-managed files stay out of version control.

9. **Propagate user-authored skills (agents-syncing)** — if `agents-syncing` is enabled in the user config, mirror skills the user placed in `<workspace>/.agents/skills/` into each configured agent's own skill directory. A skill is "user-authored" when its directory contains `SKILL.md` but lacks the `.symposium` marker (symposium never writes markers into source skills). Propagated destinations receive the same marker and `*` `.gitignore` as plugin-installed skills, so they participate in the normal stale-skill reap: removing the source — or disabling `agents-syncing` — causes the destinations to be cleaned up on the next sync. A destination directory without a marker is user-managed and is never overwritten.

10. **Reap stale skills** — across every known agent's skills parent directory, remove any subdirectory that contains the `.symposium` marker but wasn't installed this sync. Directories without the marker (user-managed) are left untouched.

11. **Register hooks** — ensure symposium's global hook handler and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config. Only symposium's own handler is registered (e.g., `cargo-agents hook claude pre-tool-use`) — individual plugin hooks are never written into agent configs. See [Hooks](./hooks.md) for the dispatch model.

## Marker file

Each skill directory symposium installs contains an empty `.symposium` file. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`, `.gemini/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker.

## Gitignore

Each skill directory symposium creates (and its `skills/` parent if new) receives a `.gitignore` containing just `*`. Pre-existing directories are left alone. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.
