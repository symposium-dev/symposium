# Discovery & sync

## TL;DR

- `symposium sync` resolves installed plugins, discovers new ones from workspace dependencies, prompts the user, fetches, evaluates predicates, and wires active content into agent directories.
- Discovery calls `list-deps` on all PMs, then passes each result as a query to `search` on all PMs.
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

The core discovery loop:

1. **Call `list-deps` on all PMs.** Each PM reports the workspace's dependencies in its ecosystem. For example, the cargo PM returns `[(cargo, serde, 1.0.210), (cargo, tokio, 1.38.0)]`. PMs that don't have a concept of workspace deps (git, path, recommendations) return empty.

2. **For each dependency, call `search` on all PMs.** The full package-id tuple is passed as the query. Each PM decides how to match:
   - The cargo PM: if `pm = cargo`, search the registry for matching plugin crates. Otherwise, return empty.
   - The recommendations PM: match on `(pm, name)`, ignoring version. If it has a `cargo/serde/` directory, it returns that as a match.
   - The git PM: returns empty (not searchable).

3. **Filter out already-installed plugins.** Compare search results against what's already in config.

4. **Prompt the user.** Present new discoveries and let them choose which to install.

5. **Record choices.** Accepted plugins are added to config. Declined plugins are recorded as dismissed.

This two-phase approach (list-deps → search) is what lets the recommendations PM "advise" on other PMs' dependencies without needing its own `list-deps` to return anything.

### The sync pipeline

`symposium sync` runs the full pipeline:

```
1. Resolve config       → installed plugin package-ids (exact versions)
2. Discover deps        → candidate plugin package-ids (via list-deps + search)
3. Prompt/auto-install  → updated installed set
4. Fetch                → populate cache
5. Evaluate predicates  → active set
6. Sync to agent dirs   → skills, hooks, MCP servers wired in
```

#### Step 1: Resolve config

Read `~/.symposium/config.toml`. For each entry, call the PM's `resolve` to get the current best match.

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
- `workspace()` → is this directory part of the workspace?
- `depends-on(cargo, axum, 0.7)` → check if cargo's `list-deps` included axum
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
2. Call `search` on all PMs with each dep.
3. If new matches exist, include in hook response:
   ```
   New extensions available for 3 dependencies. Run `symposium sync` to review.
   ```

The hook does NOT install anything. It only notifies. Installation goes through `symposium sync`.

### Auto-install configuration

```toml
# In ~/.symposium/config.toml

# Install all discoveries without prompting
auto-sync = true

# Or per-PM granularity:
[auto-sync]
recommendations = true   # auto-install from recommendations
cargo = false            # prompt for crates.io discoveries
```

### Dismissed discoveries

When a user says "none" to a discovery, it's suppressed until:
- The plugin's version changes (a new release might be more relevant)
- The user explicitly searches for it via `symposium use`

Dismissals are tracked in state (`~/.symposium/state.toml`).

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

- [ ] PR: sync pipeline with path PM

### Step 2: Discovery algorithm

Implement `list-deps` → `search` loop across all PMs. 

- [ ] PR: discovery algorithm

### Step 3: Prompt UX

Present discoveries, record choices (accept/dismiss).

- [ ] PR: discovery prompt

### Step 4: Hook notification

Add discovery check to session-start hook. Use cached results only.

- [ ] PR: session start notification

### Step 5: Auto-install and dismissal

Add `auto-sync` config, per-PM granularity, and dismissed-discovery tracking.

- [ ] PR: auto-install + dismissal state
