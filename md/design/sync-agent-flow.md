# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

0. **Consent prompt** (interactive `cargo agents sync` only) — ask about each dependency plugin awaiting consent and record the answers in `[plugins]` before resolution runs, so an approval installs in this same sync. Gated on `Output::is_interactive()`: the hook-triggered auto-sync calls `sync::sync` directly and never reaches this step. See [dependency discovery](./module-structure.md#discoveryrs--dependency-discovery-and-enablement).

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Load registries** — read the user config's `[[registry]]` entries, ask each one's package manager for the plugin-bearing entries it offers, and load their plugin manifests. For git registries, fetch/update as needed.

3. **Scan dependencies** — read the full dependency graph from the workspace.

4. **Match skills to dependencies**: for each plugin, parse `SKILL.md` YAML frontmatter, reject malformed YAML or a non-string value in a field Symposium reads, warn about skipped invalid skills, then evaluate skill group dependency predicates and individual skill `depends-on` frontmatter against the workspace dependencies.

5. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Reconcile a versioned `.symposium` marker in each installed skill directory so future syncs can recognize it as symposium-managed and bind it to local attribution metadata. A fresh install or content replacement rotates the binding, an unchanged comparison preserves it while refreshing the debounce timestamp, and a debounced pass preserves it without a write.
   - Build each install in a fresh, uniquely named `.symposium-tmp-*` directory beside its slot and rename it into place only once the copy is complete, so a failed copy leaves the slot as it was instead of an unmarked partial directory that later syncs would take for the user's. A unique name never collides with anything already there. A structured marker goes into that directory before the copy, so what a killed sync leaves behind is reaped by the next one.
   - A skill whose refresh fails still counts as wanted, so step 7 keeps its previous copy.
   - Write a `.gitignore` containing a single `*` into each installed skill directory, not its `skills/` parent, so symposium-managed files stay out of version control.

   An `[[agent]]` name this version does not know is skipped with a warning that names `cargo agents init --remove-agent`; every other agent syncs as usual.

6. **Workspace `.agents/skills/` (agents-syncing)** — not a separate step: when `agents-syncing` is enabled, each workspace plugin carries a `workspace-member()`-gated default group for `.agents/skills/`, so maintainer skills resolve and install through the same pipeline as everything else. Three guards make `.agents/skills/` safe as both a source and (for vendor-neutral agents) a destination: discovery skips `.symposium`-marked directories (installed copies are never sources), skips the marker-free `.symposium-index/` metadata directory, and leaves a skill whose source already sits at an agent's install slot in place for that agent (no self-copy, no suffixed duplicate).

7. **Reap stale skills**: across every known agent's skills parent directory, remove any subdirectory that contains the `.symposium` marker but wasn't installed this sync. Directories without the marker (user-managed) are left untouched, and so is any symlink, whatever it points to. Paths are compared resolved, so a skills directory that is a link to another agent's (`.claude/skills -> ../.agents/skills`) is scanned once and what this sync installed there is kept.

   While a registry cannot be fully read, this step is skipped and a warning names the registry: what it could not read offered nothing this time, so everything installed from it would look stale. A registry counts as not fully read when it cannot be listed or when one of its entries fails to load.

8. **Maintain attribution indexes** — after reaping each skills parent, an enabled telemetry configuration atomically publishes `.symposium-index/index.json` from the managed slots and their bindings. While a registry is unreadable, an existing enabled index is preserved alongside the installed skills; marker binding checks make stale entries fail closed. A disabled configuration removes only Symposium-owned index files, best-effort. Index metadata failure never changes the sync result.

9. **Register hooks** — ensure symposium's global hook handler and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config (only symposium's handler: a matcher group goes only once that leaves it empty), and remove the MCP entries symposium recorded for them in the [MCP ledger](./state.md#mcp-server-ledger), limited to the agent's user-level file and files inside this workspace (another workspace's project file waits for a sync there). For a configured agent, the entries recorded in this workspace's project-level MCP file that this sync no longer writes there come out as well, unless a registry cannot be read; entries in a user-level file stay, since another workspace may still offer them. Only symposium's own handler is registered (e.g., `cargo-agents hook claude pre-tool-use`); individual plugin hooks are never written into agent configs. See [Hooks](./hooks.md) for the dispatch model.

## Concurrency

Every agent session runs auto-sync on `SessionStart`, so two sessions opened together run two syncs at once. Steps 5 to 9 (skill installs, the reap, attribution-index maintenance, and agent config writes) run under an exclusive lock on `<cache>/sync.lock`, which a second sync waits for; it then finds everything up to date. Without it, one sync's reap would remove the skills another had just installed. The lock is per user rather than per workspace because at the global hook scope those steps also write user-level agent config files shared by every workspace. Resolution (steps 1 to 4) runs before the lock, since it may fetch git skill sources and a hook waiting behind that could time out.

## Marker file

Each skill directory Symposium installs contains a versioned JSON `.symposium` marker with a random local installation binding. Marker presence establishes ownership even if its contents are malformed; malformed and legacy-empty markers are repaired during sync. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets Symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker. Symposium never creates a symlink, so a link found in a skills directory is user content and is never followed or removed.

## Gitignore

Each skill directory symposium installs receives a `.gitignore` containing just `*`; parent directories such as `.claude/skills/` get none. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.

On most hook events, auto-sync is gated on `Cargo.lock` (and `battery-pack.toml`) mtime via per-workspace state, so an unchanged workspace doesn't pay for `cargo metadata` on every event. `SessionStart` is the exception: it runs once per session, ignores that gate, and passes `UpdateLevel::Check` down through skill resolution so `source.git` skill groups are re-fetched when their upstream moved. This is what makes upstream skill updates land even when the workspace's own dependencies haven't changed. `sync` takes the `UpdateLevel` as a parameter; the binary's global `--update` flag threads through the same path for manual `cargo agents sync`.

Crates reached through `[[plugins]]` chained references or `[plugins]` enablement are resolved with `UpdateLevel::Check` whatever `--update` says, so `sync` downloads one that cargo has not cached yet. The other paths that build the active plugin set (`--help`, subcommand dispatch, hook dispatch) only read the cargo cache.
