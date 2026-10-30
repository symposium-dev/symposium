# User-managed plugins

## TL;DR

- `symposium use [--global] X` searches PMs, installs a plugin, records it in config.
- `symposium use --remove X` removes that record from config.
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

$ symposium use --remove serde-skills
✓ Removed (cargo, serde-skills, 1.2.3)
```

## Detailed plans

### `symposium use [--global] <query>`

**Query:** A plugin name. A name is not an identity, since two ecosystems may use
the same word, so `use` resolves it across every PM and then decides.

**Flow:**

1. Collect every plugin the name could mean: registry plugins that name themselves, the workspace's own dependencies (checked offline, before anything reaches the network), and `search` hits from every PM, which is what lets `use` name a package the workspace does not depend on yet. Matches are deduplicated on `(pm, canonical-name)`.
2. Exactly one match is used. Several is an error naming them, which the user resolves by picking the ecosystem:
   ```
   `serde` is offered by more than one package manager:
     cargo (--pm cargo)
     symposium-recommendations (--pm symposium-recommendations)
   pick one with `--pm <name>`
   ```
3. Record the `(pm, canonical-name)` pair in config, so the entry round-trips to the same plugin.
4. Fetch into cache.
5. Run sync to activate if predicates pass.

A plugin a trust root already offers needs no entry, and `use` says so rather
than writing one. The exception is a plugin with no [activation
root](../README.md#activation-roots) of its own, where `use` is precisely the
root being supplied.

**Flags:**
- `--global`: active in all workspaces.
- Without `--global`: scoped to the current workspace directory.
- `--pm <name>`: the package manager to pick when more than one offers the name.

### `symposium use --remove <name>`

Drop the `use` entry for `<name>` and re-sync, so the plugin's content is reaped from the agent directories straight away. The cache entry stays (garbage-collected separately).

The scope has to match: without `--global` this removes the entry recorded for the current workspace, with it the unscoped one. A scope mismatch is an error rather than a silent success, since "nothing to remove" and "removed" are answers the user needs to tell apart.

Removal is the inverse of `use`, not a general off switch: it withdraws an enablement the user recorded. Turning off a plugin that was never `use`d, such as one a registry offers, is `disable`.

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
[plugins]
use = [
  # Global: active in every workspace.
  { pm = "cargo", name = "serde-skills" },
  { pm = "cargo", name = "rtk" },

  # Workspace-scoped, keyed by absolute path.
  { pm = "cargo", name = "axum-agents", workspace = "/home/user/projects/my-app" },
  { pm = "cargo", name = "diesel-helpers", workspace = "/home/user/projects/my-app" },
]
```

An entry names a plugin, not a version requirement: the pair `(pm,
canonical-name)` is the identity ([naming a plugin in
configuration](../pm-interface/README.md#naming-a-plugin-in-configuration)), and
the version is whatever the PM resolves at load time. The PM may be left out:
a bare string names whichever plugin of that name there is, and enables nothing
while it could name plugins from more than one PM.

### Scoping: global vs. local

**Global (`--global`):** Plugin activates in every workspace. Good for universally useful tools.

**Local (default):** Plugin scoped to the current workspace directory. Stored as a `use` entry carrying that absolute path.

Scope is a property of `use` only. `disable` is global, so it is not the way to
turn a plugin off in one project. See
[precedence and scope](../discovery-sync/README.md#precedence).

Key constraint: **local installs don't modify workspace files.** Scoping lives entirely in `~/.symposium/config.toml`. This means:
- No dotfiles added to the project
- Team members don't see each other's local installs
- Workspace stays clean for version control

**Workspace plugins (from `Symposium.toml`)** are a separate concept — they're project-managed, apply to all developers, and aren't touched by `use`/`remove`.

### Version updates

On each `symposium sync`, Symposium calls `load_plugin` with the configured `(pm, canonical-name)` pair. The PM finds the best matching version. Upgrades happen within the allowed range; downgrades don't.

There is no separate `symposium update` command — sync handles this naturally.

### Interaction with discovery

Discovery also writes to `[plugins]` when the user answers its prompt: approvals go to `auto-enable`, declines to `disable`. `use` entries and `auto-enable` entries both enable, and `status` shows which root a plugin came in on:

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

Workspace-scoped `use` entries record absolute paths. If you move the directory, they stop matching. Fix: update the path in config manually, or re-run `symposium use` in the new location.

### What happens when global and local plugins conflict?

If a global and local plugin provide a skill with the same name, the local one wins. `status` shows a warning.

### What if a plugin is both `use`d and disabled?

It stays off. `disable` is the last word over every enabling mechanism, so a
`use` entry naming a disabled plugin has no effect, and `use --remove` cannot
cancel a `disable`: it removes a `use` entry, which is the opposite decision.
Re-enabling means dropping the `disable` entry. See
[precedence](../discovery-sync/README.md#precedence).

### Can I install without a workspace?

`symposium use --global X` works from anywhere. Without `--global`, you need to be in a workspace directory (so Symposium knows what to scope to).

## Implementation plan and status

### Step 1: Config file format

Define and parse the `[plugins]` section: `use`, `auto-enable`, and `disable`, with global and workspace-scoped `use` entries.

- [x] PR: config format + parsing

### Step 2: `symposium use`

Search flow, selection UX, writing to config, triggering sync.

- [x] PR: `use` command

### Step 3: `symposium use --remove`

Matching, removal from config, cleanup on next sync.

- [x] PR: `remove` command

### Step 4: `symposium status`

Display installed/active/inactive plugins with provenance and predicate status.

- [x] PR: `status` command
