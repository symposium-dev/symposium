# Configuration

`cargo agents` uses a single user-wide configuration file at `~/.symposium/config.toml`. Created by `cargo agents init`.

## Full example

```toml
auto-sync = true
agents-syncing = true
hook-scope = "global"
auto-update = "on"

[[agent]]
name = "claude"

[[agent]]
name = "gemini"

[logging]
level = "info"

# Installed plugin crates
[installed]
paths = ["/home/me/dev/my-plugin-source"]
git = ["https://github.com/my-org/my-plugin-source"]

[installed.crates]
symposium-recommendations = "1"
semver-tracked = "1"
pinned-plugin = "=1.2.0"
my-org-plugins = { git = "https://github.com/my-org/my-org-plugins" }
local-dev = { path = "/home/me/dev/my-plugins" }

# Allow any workspace dep with a SYMPOSIUM.toml to be auto-discovered
[discovery]
allow = "*"
```

## Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-sync` | bool | `true` | Automatically run `cargo agents sync` during hook invocations. When enabled, skills are kept in sync with workspace dependencies without manual intervention. |
| `agents-syncing` | bool | `true` | Propagate user-authored skills from `.agents/skills/` into the per-agent skill directories of any configured agent that does not natively use `.agents/skills/` (such as `.claude/skills/` or `.kiro/skills/`). Skills that symposium itself installed — identified by the `.symposium` marker file — are not propagated. See [Workspace plugins](../workspace.md) for the user-guide overview, or [Agents syncing](#agents-syncing-mirror-user-authored-skills) below for details. |
| `hook-scope` | string | `"global"` | Where agent hooks are installed. `"global"` writes to the user's home directory (e.g., `~/`). `"project"` writes to the project directory, keeping hooks local to the workspace. |
| `auto-update` | string | `"on"` | Controls automatic update behavior. `"off"` disables update checks entirely. `"warn"` checks the registry (at most once per 24 hours) and prints a message when a newer version is available. `"on"` automatically installs the update via `cargo install` and re-executes the command with the new binary. |
| `installed` | table | `symposium-recommendations` installed crate | Registry-ready installed plugin sources. See [`[installed]`](#installed). |
| `discovery` | table | empty policy | User-configured discovery allow/deny rules. See [`[discovery]`](#discovery). |

### Agents syncing: mirror user-authored skills

Agents such as Copilot, Gemini, Codex, Goose, and OpenCode all read skills from the vendor-neutral `.agents/skills/` directory, but Claude Code and Kiro use their own paths (`.claude/skills/` and `.kiro/skills/`). When `agents-syncing` is enabled, `cargo agents sync` mirrors any skill that *you* put in `.agents/skills/` into each configured agent's own skill directory, so a single authored copy is visible to every agent.

A skill is treated as user-authored when its directory contains `SKILL.md` but does *not* contain the `.symposium` marker. Symposium never writes a marker into source skills, so the distinction between "user content" and "a copy symposium made" is unambiguous.

Propagated destinations receive the same `.symposium` marker and `*` `.gitignore` that plugin-installed skills get, which means:

- Updates to the source (`.agents/skills/<name>/`) are re-copied on each sync.
- Removing the source removes the propagated copies on the next sync (via the normal stale-skill reap).
- Disabling `agents-syncing = false` also removes previously propagated copies on the next sync.
- A pre-existing, user-managed file in the target directory (no marker) is never overwritten.

When the only configured agents use `.agents/skills/` directly, the feature is a no-op (the source and target are the same directory).

### Hook scope: control whether Symposium activates in all projects or only those you select

Registering hooks globally ensures that Symposium activates whenever you use the selected agent, which means that it will work in any Rust project automatically.

Registering hooks at the project level requires you to run `cargo agents sync` within each project at least once to create the hooks. After that, the `auto-sync` feature will keep you up-to-date.

## `[[agent]]`

Each `[[agent]]` entry identifies an agent you use. You can configure multiple agents.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | Agent name: `claude`, `codex`, `copilot`, `gemini`, `goose`, `kiro`, or `opencode`. |

## `[logging]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `level` | string | `"info"` | Minimum log level. One of: `trace`, `debug`, `info`, `warn`, `error`. |

## `[installed]`

`[installed]` declares plugin sources the user explicitly installed. New
configs include the default `symposium-recommendations = "1"` installed crate.
Legacy `[[plugin-source]]`, `[[installed-crate]]`, and `[defaults]` config
shapes are rejected.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `paths` | array of strings | `[]` | Direct path-registry plugin sources. |
| `git` | array of strings | `[]` | Direct git-registry plugin sources. |

## `[installed.crates]`

`[installed.crates]` is a Cargo dependency table keyed by crate name. Values
may be version strings or inline dependency tables with Cargo-compatible fields
such as `version`, `git`, `path`, `branch`, `tag`, `rev`, and `package`.

| Example | Meaning |
|---------|---------|
| `symposium-recommendations = "1"` | Track semver-compatible `1.x`. |
| `foo = "*"` | Track latest. |
| `foo = "=1.2.3"` | Exact pin. |
| `foo = { git = "https://github.com/me/foo" }` | Resolve through Cargo from git. |
| `foo = { path = "/home/me/foo" }` | Resolve through Cargo from a local crate. |

### Source types

- **crates.io** (name only) — Fetched via cargo. Checks for newer compatible versions on a throttled cadence (at most once per 24 hours). Exact-pinned crates (`=`) never upgrade.
- **git** — Checks for new commits on a similar throttled cadence.
- **path** — Always checks mtime; re-scans immediately if the source has changed.

## `[discovery]`

Discovery policy controls which candidate plugin sources may be activated
automatically. Rules can be wildcard shorthands or registry-specific tables:

```toml
[discovery]
allow = "*"

[discovery.deny]
crates = { unsafe-plugin = "*" }
paths = ["/tmp/untrusted"]
git = ["https://github.com/bad/*"]
```

The supported registry keys are `crates`, `paths`, and `git`. `crates = "*"`
allows or denies all crate-registry candidates; `crates = { name = "*" }`
targets individual crate names. Specific policy evaluation is part of the
resolved-source graph work and is not yet used by the legacy sync path.

## Directory resolution

User-wide data lives under `~/.symposium/` by default. Override with environment variables:

| | Config | Cache | Logs |
|---|---|---|---|
| `SYMPOSIUM_HOME` | `$SYMPOSIUM_HOME/` | `$SYMPOSIUM_HOME/cache/` | `$SYMPOSIUM_HOME/logs/` |
| XDG | `$XDG_CONFIG_HOME/symposium/` | `$XDG_CACHE_HOME/symposium/` | `$XDG_STATE_HOME/symposium/logs/` |
| Default | `~/.symposium/` | `~/.symposium/cache/` | `~/.symposium/logs/` |

`SYMPOSIUM_HOME` takes precedence over XDG variables.

## File locations

| Path | Purpose |
|------|---------|
| `~/.symposium/config.toml` | User configuration |
| `~/.symposium/state.toml` | Persistent state (binary version stamp, last update check) |
| `~/.symposium/cache/` | Cache directory (crate sources, binaries) |
| `~/.symposium/logs/` | Log files (one per invocation, timestamped) |
