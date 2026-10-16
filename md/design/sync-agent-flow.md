# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

0. **Consent prompt** (interactive `cargo agents sync` only) — ask about each dependency plugin awaiting consent and record the answers in `[plugins]` before resolution runs, so an approval installs in this same sync. Gated on `Output::is_interactive()`: the hook-triggered auto-sync calls `sync::sync` directly and never reaches this step. See [dependency discovery](./module-structure.md#discoveryrs--dependency-discovery-and-enablement).

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

## Plugins enabled for every workspace

Between matching skills (step 4) and installing them (step 5), the plugins a `use --global` entry names are compiled for the user. `hook-scope` plays no part: it decides where symposium registers its own hook and MCP servers, not where plugins go.

1. **Attribute and select**: every applicable skill knows the index of the plugin in the active set it was collected for (`SkillWithGroupContext::plugin_index`). After the usual dedup, where the first plugin to reach a skill keeps it, the skills of each plugin that `compile::installs_globally` accepts are grouped per plugin: a `[plugins] use` entry without a workspace names it (by manifest name or package name, hyphen/underscore-insensitively), and it is not a workspace member. A plugin left without skills is not compiled.
2. **Compile**: each selected plugin becomes one directory under the staging root `~/.symposium/installed/` (the `installed` subdirectory of the config dir): `plugin.json` and `.claude-plugin/plugin.json` with the same `$schema`, `name` and `version` (`0.0.0`), the `.symposium` marker, and each skill's whole directory, symlinks followed, under `skills/<skill-name>/`. The plugin name is normalized to lowercase `[a-z0-9-]`, at most 64 characters; a name two plugins share, or whose slot holds a directory symposium does not own, gets a hash suffix. The expected tree is compared with what is on disk and written to a sibling and renamed into place only when it differs. The root also carries `.claude-plugin/marketplace.json` listing every compiled plugin, for agents pointed at it as a marketplace. Marked directories not compiled this run are removed; unmarked ones are left alone.
3. **Deliver**: after its hooks and MCP servers, each configured agent's `Agent::sync_user_plugins` reconciles its user-level copy with the compiled set. When it reports that it took delivery, the per-skill loop skips those plugins' skills for that agent, so the stale-skill cleanup removes copies a previous sync left. Agents that cannot take a user-level plugin keep receiving per-skill copies in the project.
4. **Unregister**: every agent that is no longer configured is called with an empty set, by `sync` and by `init` alike, so a removed agent loses the plugins symposium installed for it.

The selection is user-level state, so every workspace compiles the same plugins, but their content is resolved in the workspace being synced: a plugin or skill that does not apply there is removed from user scope by that sync, and comes back when a workspace where it applies syncs.

## Marker file

Each skill directory symposium installs contains an empty `.symposium` file. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker.

## Gitignore

Each skill directory symposium creates (and its `skills/` parent if new) receives a `.gitignore` containing just `*`. Pre-existing directories are left alone. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.

On most hook events, auto-sync is gated on `Cargo.lock` (and `battery-pack.toml`) mtime via per-workspace state, so an unchanged workspace doesn't pay for `cargo metadata` on every event. `SessionStart` is the exception: it runs once per session, ignores that gate, and passes `UpdateLevel::Check` down through skill resolution so `source.git` skill groups are re-fetched when their upstream moved. This is what makes upstream skill updates land even when the workspace's own dependencies haven't changed. `sync` takes the `UpdateLevel` as a parameter; the binary's global `--update` flag threads through the same path for manual `cargo agents sync`.
