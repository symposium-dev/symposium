# Configuration

`cargo agents` uses a single user-wide configuration file at `~/.symposium/config.toml`. Created by `cargo agents init`.

## Full example

```toml
auto-sync = true
agents-syncing = true
hook-scope = "global"

[[agent]]
name = "claude"

[[agent]]
name = "gemini"

[logging]
level = "info"

[defaults]
symposium-recommendations = true
user-plugins = true

[[plugin-source]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"

[[plugin-source]]
name = "local-dev"
path = "my-plugins"
```

## Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-sync` | bool | `true` | Automatically run `cargo agents sync` during hook invocations. When enabled, skills are kept in sync with workspace dependencies without manual intervention. |
| `agents-syncing` | bool | `true` | Propagate user-authored skills from `.agents/skills/` into the per-agent skill directories of any configured agent that does not natively use `.agents/skills/` (such as `.claude/skills/` or `.kiro/skills/`). Skills that symposium itself installed — identified by the `.symposium` marker file — are not propagated. See [Agents syncing](#agents-syncing-mirror-user-authored-skills) below. |
| `hook-scope` | string | `"global"` | Where agent hooks are installed. `"global"` writes to the user's home directory (e.g., `~/`). `"project"` writes to the project directory, keeping hooks local to the workspace. |

### Agents syncing: mirror user-authored skills

Agents such as Copilot, Gemini, Codex, Goose, and OpenCode all read skills from the vendor-neutral `.agents/skills/` directory, but Claude Code and Kiro use their own paths (`.claude/skills/` and `.kiro/skills/`). When `agents-syncing` is enabled, `cargo agents sync` mirrors any skill that *you* put in `.agents/skills/` into each configured agent's own skill directory, so a single authored copy is visible to every agent.

A skill is treated as user-authored when its directory contains `SKILL.md` but does *not* contain the `.symposium` marker. Symposium never writes a marker into source skills, so the distinction between "user content" and "a copy symposium made" is unambiguous.

Propagated destinations receive the same `.symposium` marker and `*` `.gitignore` that plugin-installed skills get, which means:

- Updates to the source (`.agents/skills/<name>/`) are re-copied on each sync.
- Removing the source removes the propagated copies on the next sync (via the normal stale-skill reap).
- Disabling `agents-syncing = false` also removes previously propagated copies on the next sync.
- A pre-existing, user-managed file in the target directory (no marker) is never overwritten.

When the only configured agents use `.agents/skills/` directly, the feature is a no-op (the source and target are the same directory).### Hook scope: control whether Symposium activates in all projects or only those you select

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

## `[defaults]`

Controls the two built-in plugin sources. Both are enabled by default.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `symposium-recommendations` | bool | `true` | Fetch plugins from the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository. |
| `user-plugins` | bool | `true` | Scan `~/.symposium/plugins/` for user-defined plugins. |

## `[[plugin-source]]`

Defines additional plugin sources. Each entry must have exactly one of `git` or `path`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | A name for this source (used in logs and cache paths). |
| `git` | string | — | Repository URL. Fetched and cached under `~/.symposium/cache/plugin-sources/`. |
| `path` | string | — | Local directory containing plugins. Relative paths are resolved from `~/.symposium/`. |
| `auto-update` | bool | `true` | Check for updates on startup. Only applies to `git` sources. |

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
| `~/.symposium/plugins/` | User-defined plugins |
| `~/.symposium/cache/` | Cache directory (crate sources, plugin sources) |
| `~/.symposium/logs/` | Log files (one per invocation, timestamped) |
