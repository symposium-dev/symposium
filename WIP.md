# WIP: Logging Improvements

## Goal

Establish consistent, intentional logging levels across the codebase. Currently logging is sparse and inconsistently leveled. This plan defines what each level means for symposium and adds coverage to modules that have none.

## Level Definitions

- **error**: Data loss or corruption; something is broken and we can't recover
- **warn**: Non-fatal failures; we continue but something went wrong (already well-covered)
- **info**: User-meaningful milestones — network access, lifecycle events, infrequent user-initiated commands
- **debug**: Internal decision points — why something happened or was skipped; useful for troubleshooting
- **trace**: Full payloads, raw I/O, verbose internals — for deep debugging

## Key Principle

Hooks and `crate-info` are agent-initiated and high-frequency → **debug** at most.
`init`, `sync`, and `plugin` subcommands are user-initiated and infrequent → **info**.

---

## Plan by Module

### 1. `src/bin/cargo-agents.rs` (binary entry point)

Currently: no logging at all.

| Level | Event |
|-------|-------|
| info | `cargo agents init` invoked |
| info | `cargo agents sync` invoked |
| info | `cargo agents plugin {sync,list,validate}` invoked |
| debug | `cargo agents hook <agent> <event>` invoked |
| debug | `cargo agents crate-info <name>` invoked |
| trace | Full parsed CLI struct |

### 2. `src/hook.rs`

Currently: mix of debug/warn/info. Some levels need adjusting.

| Level | Current | Change |
|-------|---------|--------|
| debug | Hook listener started (agent + event) | keep |
| debug | Raw stdin input | → **trace** |
| debug | Parsed input event in `hooks_for_payload` | → **trace** |
| debug | Each hook definition checked | → **trace** |
| info | Plugin hook matched + running | → **debug** |
| info | Hook finished with child output | → **trace** |
| info | Skipping hook due to non-matching matcher | → **debug** |
| warn | All existing warn statements | keep |
| — | Auto-sync triggered or skipped | add as **debug** |
| — | Final serialized output size | add as **trace** |

### 3. `src/plugins.rs`

Currently: debug/warn, mostly fine.

| Level | Current | Change |
|-------|---------|--------|
| debug | Skipping source (auto-update disabled) | keep |
| debug | Skipping source (not git) | keep |
| debug | Ensuring plugin source (url) | keep |
| debug | Plugin source ready | keep |
| debug | Loaded plugin from TOML | keep |
| debug | Found standalone skill | keep |
| warn | Failed to fetch/load | keep |
| — | Scan summary (N plugins, N skills found) | add as **debug** |

### 4. `src/sync.rs`

Currently: no logging (uses `Output` for user-facing messages only).

| Level | Event |
|-------|-------|
| info | Sync started (N workspace deps, N agents) |
| debug | Workspace root resolved to path |
| debug | Skill not installed (crates not in workspace) |
| debug | Manifest loaded/saved |
| info | Skill installed (name → agent dir) |
| info | Stale skill removed |
| info | Hooks registered/unregistered for agent |
| info | MCP servers registered/unregistered for agent |
| debug | No applicable skills found |

### 5. `src/init.rs`

Currently: no logging.

| Level | Event |
|-------|-------|
| info | Init started |
| info | Config written (agents, scope) |
| debug | Resolved agents list |
| debug | Hook scope selected |

### 6. `src/config.rs`

Currently: no runtime logging (just init_logging setup).

| Level | Event |
|-------|-------|
| debug | Config loaded from path (or default) |
| debug | Resolved config/cache/logs dirs |
| debug | Effective log level |
| trace | Full parsed Config struct |

### 7. `src/crate_command.rs` + `src/crate_sources/`

Currently: no logging.

| Level | Event |
|-------|-------|
| debug | `crate-info` dispatched (name, version constraint) |
| debug | Version resolved (workspace vs crates.io) |
| debug | Cache hit for extracted crate |
| info | Crate source downloaded from crates.io (name + version) |
| trace | `crate-info` formatted output |
| trace | Tarball download size, extraction details |

### 8. `src/git_source.rs`

Currently: debug/info/warn, mostly good.

| Level | Current | Change |
|-------|---------|--------|
| debug | Cache recent, skipping check | keep |
| info | Force-fetching | keep |
| debug | Cache is fresh (SHA matches) | keep |
| info | Cache stale, re-fetching | keep |
| warn | Failed freshness check, using cache | keep |
| — | GitHub API request URL | add as **trace** |
| — | Tarball download size | add as **trace** |
| — | Cache meta written | add as **trace** |

### 9. `src/agents/` (hook + MCP registration)

Currently: no logging (uses `Output` only).

| Level | Event |
|-------|-------|
| debug | Hook registration: already registered (skip) |
| debug | Hook registration: added hooks (events) |
| debug | MCP server: already correct (skip) |
| debug | MCP server: inserted/updated |
| trace | Full JSON written to agent config |

### 10. `src/skills.rs`

Currently: warn only.

| Level | Event |
|-------|-------|
| debug | Group crate predicates don't match, skipping |
| debug | Skill loaded (name + crates) |
| debug | Skill matches workspace crate |
| trace | Full frontmatter parsed |
| warn | Existing warns | keep |

---

## Implementation Order

1. **hook.rs** — fix existing levels (info→debug, debug→trace)
2. **bin/cargo-agents.rs** — add entry point logging
3. **sync.rs** — add info/debug logging
4. **init.rs** — add info/debug logging
5. **config.rs** — add debug/trace logging
6. **plugins.rs** — add scan summary debug line
7. **skills.rs** — add debug logging
8. **crate_command.rs + crate_sources/** — add debug/info/trace
9. **agents/** — add debug/trace logging
10. **git_source.rs** — add trace logging
