# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Load registries** — read the user config's `[[registry]]` entries, ask each one's package manager for the plugin-bearing entries it offers, and load their plugin manifests. For git registries, fetch/update as needed.

3. **Scan dependencies** — read the full dependency graph from the workspace.

4. **Match skills to dependencies** — for each plugin, parse `SKILL.md` YAML frontmatter, reject malformed or non-string metadata, warn about skipped invalid skills, then evaluate skill group dependency predicates and individual skill `depends-on` frontmatter against the workspace dependencies.

5. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Drop a `.symposium` marker file into each installed skill directory so future syncs (and other tools) can recognize it as symposium-managed.
   - For every skill directory symposium creates along the way (the skill directory itself or its `skills/` parent), write a `.gitignore` containing a single `*` so symposium-managed files stay out of version control.

6. **Workspace `.agents/skills/` (agents-syncing)** — not a separate step: when `agents-syncing` is enabled, each workspace plugin carries a `workspace-member()`-gated default group for `.agents/skills/`, so maintainer skills resolve and install through the same pipeline as everything else. Two marker guards make `.agents/skills/` safe as both a source and (for vendor-neutral agents) a destination: discovery skips `.symposium`-marked directories (installed copies are never sources), and a skill whose source already sits at an agent's install slot is skipped for that agent (no self-copy, no suffixed duplicate).

7. **Reap stale skills** — across every known agent's skills parent directory, remove any subdirectory that contains the `.symposium` marker but wasn't installed this sync. Directories without the marker (user-managed) are left untouched.

8. **Register hooks** — ensure symposium's global hook handler and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config. Only symposium's own handler is registered (e.g., `cargo-agents hook claude pre-tool-use`) — individual plugin hooks are never written into agent configs. See [Hooks](./hooks.md) for the dispatch model.

## Marker file

Each skill directory symposium installs contains an empty `.symposium` file. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`, `.gemini/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker.

## Gitignore

Each skill directory symposium creates (and its `skills/` parent if new) receives a `.gitignore` containing just `*`. Pre-existing directories are left alone. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.

On most hook events, auto-sync is gated on `Cargo.lock` (and `battery-pack.toml`) mtime via per-workspace state, so an unchanged workspace doesn't pay for `cargo metadata` on every event. `SessionStart` is the exception: it runs once per session, ignores that gate, and passes `UpdateLevel::Check` down through skill resolution so `source.git` skill groups are re-fetched when their upstream moved. This is what makes upstream skill updates land even when the workspace's own dependencies haven't changed. `sync` takes the `UpdateLevel` as a parameter; the binary's global `--update` flag threads through the same path for manual `cargo agents sync`.
