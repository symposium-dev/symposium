# User-managed plugins

## TL;DR

- `symposium use [--global] X` searches PMs, installs a plugin, records it in config.
- `symposium remove X` removes from config.
- `symposium status` shows what's installed, what's active, and why.
- Global installs apply everywhere; local installs are scoped to a workspace directory without modifying workspace files.

## Motivation

Users need to explicitly manage plugins: install tools they've heard about, remove ones they don't want, and understand what's active. The UX should be as familiar as `cargo install` or `npm install -g` — search, pick, done.

## Change in a nutshell

```bash
$ symposium use serde-skills
Found plugins matching "serde-skills":

  [1] (cargo, serde-skills, 1.2.3) — Schema-aware serialization helpers

Install? [1]: 1
✓ Installed (cargo, serde-skills, 1.2.3)
✓ Active (depends-on(cargo, serde, 1.0) matches in this workspace)

$ symposium status
Installed plugins:

  (cargo, serde-skills, 1.2.3) [local: ~/projects/my-app]
    Active: yes
    Skills: serde-usage, serde-derive-helper

$ symposium remove serde-skills
✓ Removed (cargo, serde-skills, 1.2.3)
```

## Detailed plans

### `symposium use [--global] <query>`

**Query:** A name or partial identifier. Symposium searches all PMs for matches.

**Flow:**

1. Call `search` on all PMs with the query.
2. One result → confirm and install. Multiple → present selection:
   ```
   Found plugins matching "serde":
     [1] (cargo, serde-skills, 1.2.3) — Schema-aware serialization helpers
     [2] (recommendations, cargo/serde, 0.1.0) — Recommended serde extensions
   Install which? [1]:
   ```
3. Record in config.
4. Fetch into cache.
5. Run sync to activate if predicates pass.

**Flags:**
- `--global` — active in all workspaces.
- Without `--global` — scoped to the current workspace directory.

### `symposium remove <query>`

Match `<query>` against installed plugins. If ambiguous, prompt. Remove from config. On next sync, content is cleaned from agent directories. Cache entry stays (garbage-collected separately).

### `symposium status`

Shows installed plugins grouped by scope, with activation status:

```
Global plugins:
  (cargo, rtk, 2.1.0)
    Active: yes
    Skills: rtk-reduce, rtk-expand

Local plugins (~/projects/my-app):
  (cargo, axum-agents, 0.5.1)
    Active: yes (workspace-dependency() ✓)
    Skills: axum-routing, axum-testing

  (cargo, diesel-helpers, 1.0.0)
    Active: no (workspace-dependency() ✗)
    Source: discovery (auto-installed 2026-05-15)

Workspace plugins (from Symposium.toml):
  Skills: project-guide, testing-conventions
```

### Config file format

Location: `~/.symposium/config.toml`

```toml
# Global plugins
[[plugins]]
source.cargo = { serde-skills = "1" }

[[plugins]]
source.cargo = { rtk = "2" }

# Workspace-scoped plugins
[[workspace-plugins]]
directory = "/home/user/projects/my-app"
source.cargo = { axum-agents = "0.5" }

[[workspace-plugins]]
directory = "/home/user/projects/my-app"
source.cargo = { diesel-helpers = "1" }
```

Note: config entries use `source.<pm>` syntax — the same format as `Symposium.toml` plugin entries. Symposium passes the value to the PM's `resolve` to get the exact package-id. The version in the source value is a *requirement* (e.g., `"1"` means any 1.x); the resolved package-id has the exact version.

### Scoping: global vs. local

**Global (`--global`):** Plugin activates in every workspace. Good for universally useful tools.

**Local (default):** Plugin scoped to the current workspace directory. Stored as `[[workspace-plugins]]` keyed by absolute path.

Key constraint: **local installs don't modify workspace files.** Scoping lives entirely in `~/.symposium/config.toml`. This means:
- No dotfiles added to the project
- Team members don't see each other's local installs
- Workspace stays clean for version control

**Workspace plugins (from `Symposium.toml`)** are a separate concept — they're project-managed, apply to all developers, and aren't touched by `use`/`remove`.

### Version updates

On each `symposium sync`, Symposium calls `resolve` with the source value from config. The PM finds the best matching version. Upgrades happen within the allowed range; downgrades don't.

There is no separate `symposium update` command — sync handles this naturally.

### Interaction with discovery

Discovery can also add entries to config (when the user accepts a discovered plugin during sync). These show up as regular `[[plugins]]` or `[[workspace-plugins]]` entries. The `status` command shows provenance:

```
Source: discovery (auto-installed 2026-05-15)
```

vs.

```
Source: symposium use axum-agents
```

Both are equivalent in config. The distinction is informational.

## Frequently asked questions

### Why not modify workspace files for local installs?

Local installs are personal preferences. Putting them in workspace files would commit them to version control, affecting the whole team. The `Symposium.toml` in the workspace is for team-wide plugins; `~/.symposium/config.toml` is for personal ones.

### What if I move my project directory?

`[[workspace-plugins]]` entries use absolute paths. If you move the directory, they stop matching. Fix: update the path in config manually, or re-run `symposium use` in the new location.

### What happens when global and local plugins conflict?

If a global and local plugin provide a skill with the same name, the local one wins. `status` shows a warning.

### Can I install without a workspace?

`symposium use --global X` works from anywhere. Without `--global`, you need to be in a workspace directory (so Symposium knows what to scope to).

## Implementation plan and status

### Step 1: Config file format

Define and parse the `[[plugins]]` and `[[workspace-plugins]]` entries in config.

- [ ] PR: config format + parsing

### Step 2: `symposium use`

Search flow, selection UX, writing to config, triggering sync.

- [ ] PR: `use` command

### Step 3: `symposium remove`

Matching, removal from config, cleanup on next sync.

- [ ] PR: `remove` command

### Step 4: `symposium status`

Display installed/active/inactive plugins with provenance and predicate status.

- [ ] PR: `status` command
