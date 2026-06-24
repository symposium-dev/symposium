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
[[installed-crate]]
name = "symposium-recommendations"

[[installed-crate]]
name = "my-org-plugins"
git = "https://github.com/my-org/my-org-plugins"

[[installed-crate]]
name = "semver-tracked"
version = "1"

[[installed-crate]]
name = "local-dev"
path = "/home/me/dev/my-plugins"

# Allow any workspace dep with a SYMPOSIUM.toml to be auto-discovered
dependency-allow-list = ["*"]
```

## Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-sync` | bool | `true` | Automatically run `cargo agents sync` during hook invocations. When enabled, skills are kept in sync with workspace dependencies without manual intervention. |
| `agents-syncing` | bool | `true` | Propagate user-authored skills from `.agents/skills/` into the per-agent skill directories of any configured agent that does not natively use `.agents/skills/` (such as `.claude/skills/` or `.kiro/skills/`). Skills that symposium itself installed — identified by the `.symposium` marker file — are not propagated. See [Workspace plugins](../workspace.md) for the user-guide overview, or [Agents syncing](#agents-syncing-mirror-user-authored-skills) below for details. |
| `hook-scope` | string | `"global"` | Where agent hooks are installed. `"global"` writes to the user's home directory (e.g., `~/`). `"project"` writes to the project directory, keeping hooks local to the workspace. |
| `auto-update` | string | `"on"` | Controls automatic update behavior. `"off"` disables update checks entirely. `"warn"` checks the registry (at most once per 24 hours) and prints a message when a newer version is available. `"on"` automatically installs the update via `cargo install` and re-executes the command with the new binary. |
| `dependency-allow-list` | array of strings | `[]` | Workspace dependencies that are approved for automatic plugin discovery. When a workspace dep appears in this list and contains a `SYMPOSIUM.toml`, it is treated as an installed plugin crate. Use `["*"]` to approve all workspace deps. Combines with allow lists declared by installed plugin crates. |

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

## `[[installed-crate]]`

Each `[[installed-crate]]` entry declares a plugin crate to load. Managed by `cargo agents install` / `cargo agents uninstall`, but can also be edited manually. Each entry must have exactly one of: a `name` only (crates.io), `git`, or `path`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | Crate name. |
| `version` | string | — | Version requirement (Cargo semver syntax). `"1"` means `^1`, `"=1.2.3"` means exact pin. If omitted, tracks latest. |
| `git` | string | — | Git repository URL. |
| `path` | string | — | Local directory path. |

### Source types

- **crates.io** (name only) — Fetched via cargo. Checks for newer compatible versions on a throttled cadence (at most once per 24 hours). Exact-pinned crates (`=`) never upgrade.
- **git** — Checks for new commits on a similar throttled cadence.
- **path** — Always checks mtime; re-scans immediately if the source has changed.

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
