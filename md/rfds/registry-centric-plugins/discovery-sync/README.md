# Discovery & sync

## TL;DR

- `symposium sync` resolves installed plugins, discovers new ones from workspace dependencies, prompts the user, fetches, evaluates predicates, and wires active content into agent directories.
- Every PM is asked the same two things: `list-deps`, then `active_plugins` over that dependency set. What differs is the answer's source: a trusted PM's plugins load directly, an untrusted PM's need the user's consent first.
- A session-start hook notifies users of available extensions without auto-installing.

## Motivation

Users shouldn't have to manually find and install plugins for every crate they depend on. Discovery bridges the gap: when you add `serde` to your `Cargo.toml`, Symposium notices and offers relevant extensions. The sync pipeline ensures everything stays consistent.

## Change in a nutshell

User adds `axum` to their project. On next agent session start, they see:

```
New extensions available for 1 dependency. Run `symposium sync` to review.
```

They run `symposium sync`:

```
New extensions available:

  [1] (cargo, axum-agents, 0.5.1) — Route documentation and testing skills
      (because you depend on axum)

Install? [1,all,none]: 1
✓ Installed (cargo, axum-agents, 0.5.1)
✓ Synced 2 skills: axum-routing, axum-testing
```

## Detailed plans

### The discovery algorithm

The core loop:

1. **Call `list-deps` on all PMs.** Each PM reports the workspace's dependencies in its ecosystem. For example, the cargo PM returns `[(cargo, serde, 1.0.210), (cargo, tokio, 1.38.0)]`. PMs with no notion of workspace deps (a registry) return empty.

2. **Call `active_plugins` on all PMs**, passing that dependency set. A registry answers with its own entries, ignoring the deps. An ecosystem transport answers with the plugins its dependencies embed: the crate that ships a `skills/` directory or a `Symposium.toml` of its own. Fetching is cache-only here, so this makes no network calls and a workspace dependency is inspected in the source the PM already extracted.

3. **Split the plugins by the source they came from.** A registry is a trust root, so its plugins are loaded straight away and gated by nothing but their own predicates. A dependency is not: depending on a package means compiling its code, not letting its author add to the agent's context, so a plugin embedded in one needs the user's say-so. Only these reach the next step.

4. **Classify each remaining plugin against `[plugins]`.** A name may be enabled by `use`, pre-consented by `auto-enable`, previously declined via `disable`, or undecided, which makes it a *candidate*.

5. **Prompt the user** about the candidates, and record the answers: approvals into `auto-enable`, declines into `disable`.

So every PM is asked, and asked the same thing. Trust does not decide who gets
asked; it decides what happens to the answer. Steps 3 to 5 are what "discovery"
names in the narrow sense, the consent decision, and only dependency-embedded
plugins ever need one. Nothing here fetches or writes until the prompt is
answered.

Note what is *not* here: no per-dependency `search`. Curated recommendations do
not need one, because a recommendation is an ordinary registry plugin that names
the crates it advises on with `depends-on`, which the ordinary predicate pass
already evaluates. `search` is a user-facing lookup, backing `symposium use` and
`symposium search`, where the input is a partial name typed by a person rather
than a package-id.

### The sync pipeline

`symposium sync` runs the full pipeline:

```
1. Resolve config       → installed plugin package-ids (exact versions)
2. Discover deps        → candidate plugin package-ids (via list-deps + active_plugins)
3. Prompt/auto-install  → updated installed set
4. Fetch                → populate cache
5. Evaluate predicates  → active set
6. Sync to agent dirs   → skills, hooks, MCP servers wired in
```

#### Step 1: Resolve config

Read `~/.symposium/config.toml`. Each `[plugins]` entry names a `(pm, canonical-name)` pair; `load_plugin` on the owning PM turns it into the current best match.

#### Step 2: Discover deps

Run the discovery algorithm described above.

#### Step 3: Prompt or auto-install

Present new discoveries to the user:

```
New extensions available:

  [1] (cargo, serde-skills, 1.2.3) — Schema-aware serialization helpers
      (because you depend on serde)

  [2] (cargo, axum-agents, 0.5.1) — Route documentation and testing skills
      (because you depend on axum)

Install? [1,2,all,none]:
```

If `auto-sync = true` in config, skip the prompt and install all.

Selected plugins are added to config. Declined plugins are recorded as dismissed.

#### Step 4: Fetch

For each installed plugin, call `fetch` on its PM to populate the cache. Chained plugins declared in a plugin's `Symposium.toml` are fetched transitively.

Fetching happens in parallel across PMs and packages.

#### Step 5: Evaluate predicates

For each cached plugin, evaluate its predicates against the workspace:
- `workspace-member()` → is this plugin defined by a member of the workspace?
- `depends-on(axum>=0.7)` → did some PM's `list-deps` include a matching axum?
- etc.

Plugins that pass are *active*. Plugins that don't pass are installed but dormant.

#### Step 6: Sync to agent dirs

Copy active skills/hooks/MCP servers into agent directories. Same change-awareness as today:
- Compare source and destination content
- Only write when files differ
- Clean up stale entries from deactivated/removed plugins

### Hook-triggered notification

On session start, a lightweight check runs:

1. Use cached `list-deps` results (from lockfile mtime — no network calls).
2. Run discovery over them, cache-only.
3. If undecided candidates exist, include in hook response:
   ```
   New extensions available for 3 dependencies. Run `symposium sync` to review.
   ```

The hook does NOT install anything. It only notifies. Installation goes through `symposium sync`.

### Enablement configuration

Enablement is keyed on `(pm, canonical-name)`, the identity every PM gives the
plugins it offers (see the [PM interface](../pm-interface/README.md#naming-a-plugin-in-configuration)).
The pair is what lets a user name one specific plugin: a crate for the cargo PM,
an entry path for a registry PM, and never an ambiguous bare word.

```toml
# In ~/.symposium/config.toml

[plugins]
# Pre-consented, so a discovery installs without prompting.
auto-enable = [{ pm = "cargo", name = "my-internal-crate" }]

# Deliberate enablements, global or scoped to one workspace.
use = [
  { pm = "cargo", name = "widget" },
  { pm = "cargo", name = "gadget", workspace = "/path/to/project" },
]

# Pruned from enablement, which is also where a decline is recorded, and how a
# plugin from a trusted source is turned off.
disable = [{ pm = "symposium-recommendations", name = "rtk" }]
```

`auto-enable` also accepts `"*"`, meaning every dependency-embedded plugin is
consented to. `disable` still applies on top, so blanket consent stays
overridable one plugin at a time.

#### Precedence

The three lists answer different questions, so they can name the same plugin at
once. The rule is that **`disable` wins**, unconditionally:

| Configuration | Result |
|---------------|--------|
| `use` only | enabled, subject to its predicates |
| `auto-enable` only | enabled if a dependency embeds it |
| `use` + `auto-enable` | enabled; `use` additionally reaches a plugin no dependency embeds |
| anything + `disable` | off |

`disable` has to be the last word to be worth having. Everything else (a trusted
registry, `auto-enable`, an explicit `use`) is a way of saying a plugin *may*
run, and `disable` is the only way to say it may not; a precedence rule
that let any of them beat it would mean there is no way to turn a plugin off.

So `use` on a disabled plugin does not re-enable it, and `symposium use --remove`
does not cancel a `disable` (it removes a `use` entry, which is the opposite
decision). Re-enabling means dropping the `disable` entry.

#### Scope

`use` carries a scope: an entry is either global or recorded for one workspace
root, so a plugin can be enabled in the one project that wants it.

`auto-enable` and `disable` do not: both are global. For `auto-enable` this is a
consequence of what it means, namely standing consent to what your dependencies
carry, which is a judgment about the plugin's author rather than about a
project. For `disable` it is a simplification worth naming: turning a plugin off in one
workspace turns it off in all of them. See the parent RFD's
[future work](../README.md#future-work).

### Declined discoveries

A decline is recorded in `[plugins] disable` in the user config, and is
permanent until the user edits it. It lives in config rather than state because
it is a decision the user made and should be able to see and revise, not a
cache Symposium is free to invalidate; a version bump does not re-raise it.

Only an explicit "never ask again" is written. The prompt's default answer
("ask me later") and Escape record nothing, so hitting Enter reflexively never
declines anything permanently.

The prompt is inert unless the output is attached to a terminal on both ends. A
hook must never block on stdin, so on the hook path the pending candidates are
rendered into `SessionStart` context pointing at `cargo agents sync` instead.

### Debouncing and caching

- `list-deps` results are cached based on lockfile mtime. No cargo invocation if `Cargo.lock` hasn't changed.
- Discovery search results are cached with a 24-hour TTL.
- The session-start hook path uses cached results exclusively — no network calls during hook handling.

## Frequently asked questions

### Why not auto-install by default?

Installing code without consent is a security concern. Users should see what's being proposed and approve it. The `auto-sync = true` opt-in is for users who trust the recommendations set and want zero friction.

### Why only direct dependencies?

Transitive deps are numerous and usually not relevant to the user's workflow. Direct deps keep discovery focused.

### What if `list-deps` is slow?

The cargo PM's `list-deps` reads `Cargo.lock` directly (fast parse). The result is cached on lockfile mtime. In the common case (lock unchanged), `list-deps` is a no-op.

### Can discovery be disabled entirely?

Yes: `auto-sync = false` (the default) means you only get notified, never auto-installed. To suppress even the notification, set `discovery = false` in config.

## Implementation plan and status

### Step 1: Sync pipeline skeleton

Wire up the pipeline with the path PM initially to validate the flow end-to-end.

- [x] PR: sync pipeline with path PM

### Step 2: Discovery algorithm

Implement `list-deps` → `search` loop across all PMs. 

- [x] PR: discovery algorithm

### Step 3: Prompt UX

Present discoveries, record choices (accept/dismiss).

- [x] PR: discovery prompt

### Step 4: Hook notification

Add discovery check to session-start hook. Use cached results only.

- [x] PR: session start notification

### Step 5: Auto-install and dismissal

Add `auto-sync` config, per-PM granularity, and dismissed-discovery tracking.

- [x] PR: auto-install + dismissal state
