# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Load plugin sources** — read the user config's `[[plugin-source]]` entries and load their plugin manifests. For git sources, fetch/update as needed.

3. **Scan dependencies** — read the full dependency graph from the workspace.

4. **Match skills to dependencies** — for each plugin, parse `SKILL.md` YAML frontmatter, reject malformed or non-string metadata, warn about skipped invalid skills, then evaluate skill group crate predicates and individual skill `crates` frontmatter against the workspace dependencies.

5. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Drop a `.symposium` marker file into each installed skill directory so future syncs (and other tools) can recognize it as symposium-managed.
   - For every skill directory symposium creates along the way (the skill directory itself or its `skills/` parent), write a `.gitignore` containing a single `*` so symposium-managed files stay out of version control.

6. **Reap stale skills** — across every known agent's skills parent directory, remove any subdirectory that contains the `.symposium` marker but wasn't installed this sync. Directories without the marker (user-managed) are left untouched.

7. **Register hooks** — ensure global hooks and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config.

## Marker file

Each skill directory symposium installs contains an empty `.symposium` file. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`, `.gemini/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker.

## Gitignore

Each skill directory symposium creates (and its `skills/` parent if new) receives a `.gitignore` containing just `*`. Pre-existing directories are left alone. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.
