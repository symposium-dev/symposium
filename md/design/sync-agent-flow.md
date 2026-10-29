# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

0. **Consent prompt** (interactive `cargo agents sync` only) — ask about each dependency plugin awaiting consent and record the answers in `[plugins]` before resolution runs, so an approval installs in this same sync. Gated on `Output::is_interactive()`: the hook-triggered auto-sync calls `sync::sync` directly and never reaches this step. See [dependency discovery](./module-structure.md#discoveryrs--dependency-discovery-and-enablement).

1. **Find the workspace**: ask each package manager for the project it finds from the starting directory (for a hook, the payload's `cwd`; for `cargo agents sync`, the current directory): its root, members and inputs, through `project_info`. Outside any project there is nothing to sync; package managers that disagree about the root are an error. For cargo this is the Cargo workspace, from `cargo metadata`.

2. **Load registries** — read the user config's `[[registry]]` entries, ask each one's package manager for the plugin-bearing entries it offers, and load their plugin manifests. For git registries, fetch/update as needed.

3. **Scan dependencies**: every package manager's `list_deps`, unioned.

4. **Match skills to dependencies**: for each plugin, parse `SKILL.md` YAML frontmatter, reject malformed YAML or a non-string value in a field Symposium reads, warn about skipped invalid skills, then evaluate skill group dependency predicates and individual skill `depends-on` frontmatter against the workspace dependencies.

5. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Drop a `.symposium` marker file into each installed skill directory so future syncs (and other tools) can recognize it as symposium-managed.
   - Build each install in a fresh, uniquely named `.symposium-tmp-*` directory beside its slot and rename it into place only once the copy is complete, so a failed copy leaves the slot as it was instead of an unmarked partial directory that later syncs would take for the user's. A unique name never collides with anything already there. The marker goes into that directory before the copy, so what a killed sync leaves behind is reaped by the next one.
   - A skill whose refresh fails still counts as wanted, so step 7 keeps its previous copy.
   - Write a `.gitignore` containing a single `*` into each installed skill directory, not its `skills/` parent, so symposium-managed files stay out of version control.

   An `[[agent]]` name this version does not know is skipped with a warning that names `cargo agents init --remove-agent`; every other agent syncs as usual.

6. **Workspace `.agents/skills/` (agents-syncing)** — not a separate step: when `agents-syncing` is enabled, each workspace plugin carries a `workspace-member()`-gated default group for `.agents/skills/`, so maintainer skills resolve and install through the same pipeline as everything else. Two marker guards make `.agents/skills/` safe as both a source and (for vendor-neutral agents) a destination: discovery skips `.symposium`-marked directories (installed copies are never sources), and a skill whose source already sits at an agent's install slot is skipped for that agent (no self-copy, no suffixed duplicate).

7. **Reap stale skills**: across every known agent's skills parent directory, remove any subdirectory that contains the `.symposium` marker but wasn't installed this sync. Directories without the marker (user-managed) are left untouched, and so is any symlink, whatever it points to. Paths are compared resolved, so a skills directory that is a link to another agent's (`.claude/skills -> ../.agents/skills`) is scanned once and what this sync installed there is kept.

   While a registry cannot be fully read, this step is skipped and a warning names the registry: what it could not read offered nothing this time, so everything installed from it would look stale. A registry counts as not fully read when it cannot be listed or when one of its entries fails to load.

8. **Register hooks**: ensure symposium's global hook handler and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config (only symposium's handler: a matcher group goes only once that leaves it empty), and remove the MCP entries symposium recorded for them in the [MCP ledger](./state.md#mcp-server-ledger), limited to the agent's user-level file and files inside this workspace (another workspace's project file waits for a sync there). For a configured agent, the entries recorded in this workspace's project-level MCP file that this sync no longer writes there come out as well, unless a registry cannot be read; entries in a user-level file stay, since another workspace may still offer them. Only symposium's own handler is registered (e.g., `cargo-agents hook claude pre-tool-use`); individual plugin hooks are never written into agent configs. See [Hooks](./hooks.md) for the dispatch model.

## Concurrency

Every agent session runs auto-sync on `SessionStart`, so two sessions opened together run two syncs at once. Steps 5 to 8 (skill installs, the reap, and agent config writes) run under an exclusive lock on `<cache>/sync.lock`, which a second sync waits for; it then finds everything up to date. Without it, one sync's reap would remove the skills another had just installed. The lock is per user rather than per workspace because at the global hook scope those steps also write user-level agent config files shared by every workspace. Resolution (steps 1 to 4) runs before the lock, since it may fetch git skill sources and a hook waiting behind that could time out.

## Marker file

Each skill directory symposium installs contains an empty `.symposium` file. Cleanup walks every agent's skills parent directory (`.claude/skills/`, `.agents/skills/`, `.kiro/skills/`) and reaps any subdirectory whose marker is present but which wasn't installed this sync. This lets symposium reclaim stale skills (including those left behind by agents removed from the config) without touching user-managed skills, which are identified by the absence of the marker. Symposium never creates a symlink, so a link found in a skills directory is user content and is never followed or removed.

## Gitignore

Each skill directory symposium installs receives a `.gitignore` containing just `*`; parent directories such as `.claude/skills/` get none. The wildcard also hides the marker file and the gitignore itself, so `git status` stays clean.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.

On most hook events, auto-sync is gated on per-workspace state: each package manager names the files its answers depend on (`ProjectInfo::inputs`; for cargo, `Cargo.lock` and `battery-pack.toml`), a sync records their mtimes and the workspace found, keyed on the directory the package managers started from, and a later event syncs only once one of those files has changed, appeared, or disappeared. Checking that reads only mtimes, so an unchanged workspace starts no package manager and runs no `cargo metadata`; the recorded workspace is reused instead. Nothing is recorded outside a project, so the next event asks again. A sync is recorded even when a package manager failed: its cleanup was skipped, and it is asked again at the next `SessionStart` or input change. `SessionStart` is the exception: it runs once per session, ignores that gate, and passes `UpdateLevel::Check` down through skill resolution so `source.git` skill groups are re-fetched when their upstream moved. This is what makes upstream skill updates land even when the workspace's own dependencies haven't changed. `sync` takes the `UpdateLevel` as a parameter; the binary's global `--update` flag threads through the same path for manual `cargo agents sync`.

Crates reached through `[[plugins]]` chained references or `[plugins]` enablement are resolved with `UpdateLevel::Check` whatever `--update` says, so `sync` downloads one that cargo has not cached yet. The other paths that build the active plugin set (`--help`, subcommand dispatch, hook dispatch) only read the cargo cache.
