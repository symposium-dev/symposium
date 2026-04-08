# Configuration

`symposium` uses two configuration files: a **user-wide** config and an optional **per-project** config. When both exist, project settings override user settings.

## User configuration

Stored at `~/.symposium/config.toml`. Created by `symposium init --user`.

### Full example

```toml
[agent]
name = "claude-code"
sync-default = true
auto-sync = true

[logging]
level = "info"

[[plugin-source]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"

[[plugin-source]]
name = "local-dev"
path = "~/my-plugins"
```

### `[agent]`

Your agent preference and default behaviors.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | Which agent you use (e.g., `"claude-code"`, `"cursor"`). |
| `sync-default` | bool | `true` | Default on/off for newly discovered extensions. |
| `auto-sync` | bool | `false` | Automatically run `sync --workspace` when workspace dependencies change. Detected by comparing the mtime of `Cargo.lock` against a cached value. |

### `[logging]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `level` | string | `"info"` | Minimum log level. One of: `trace`, `debug`, `info`, `warn`, `error`. |

### `[[plugin-source]]`

Defines where `symposium` looks for skills, workflows, and MCP server definitions. Each entry must have exactly one of `git` or `path`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | A name for this source (used in logs and cache paths). |
| `git` | string | — | Repository URL. Fetched and cached under `~/.symposium/cache/plugin-sources/`. |
| `path` | string | — | Local directory containing plugins. |
| `auto-update` | bool | `true` | Check for updates on startup. Only applies to `git` sources. |

## Project configuration

Stored at `.symposium/config.toml` in your project root. Created by `symposium init --project` and updated by `symposium sync`.

### Full example

```toml
[agent]
name = "claude"
sync-default = false

self-contained = false

[defaults]
symposium-recommendations = true
user-plugins = true

[[plugin-source]]
name = "our-team"
git = "https://github.com/our-org/symposium-plugins"

[[plugin-source]]
name = "local"
path = "plugins"

[skills]
salsa = true
tokio = true
serde = false

[workflows]
rtk = true
autofmt = true
```

### `[agent]`

Optional. If present, overrides the user's agent settings for this project. Supports the same keys as the user-level `[agent]` section.

If omitted, each developer uses their own user-wide agent preference.

### `self-contained`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `self-contained` | bool | `false` | If `true`, ignore user-level plugin sources entirely. Only project sources (including its own `[defaults]` and `[[plugin-source]]` entries) are used. |

### `[defaults]`

Optional. Controls built-in plugin sources at the project level. Same keys as the user-level `[defaults]` section.

When `self-contained = false` (the default), project defaults are merged with user defaults — a project `false` overrides a user `true`. When `self-contained = true`, only the project defaults apply.

### `[[plugin-source]]`

Project-level plugin sources. Same format as user-level `[[plugin-source]]` entries. Paths are resolved relative to the project root.

When `self-contained = false`, these are unioned with user-level sources. When `self-contained = true`, these are the only sources used (along with any enabled defaults).

### `[skills]`

Lists available crate skills discovered from your workspace dependencies. Each key is a crate name, each value is a bool toggling the skill on or off.

Managed by `symposium sync --workspace` — new entries are added with the resolved `sync-default`, removed dependencies are cleaned up, and your existing choices are preserved.

### `[workflows]`

Lists available workflow extensions. Same format as `[skills]`: each key is a workflow name, each value is a bool.

## Setting resolution

When both user and project configs exist, project settings take precedence:

| Setting | Resolution |
|---------|------------|
| `agent.name` | Project if set, else user |
| `agent.sync-default` | Project if set, else user |
| `agent.auto-sync` | Project if set, else user |
| Plugin sources | Union of user + project (or project only if `self-contained`) |
| Defaults | Merged (project `false` overrides user `true`; project only if `self-contained`) |
| Skills, workflows | Project config only |

## Directory resolution

User-wide data lives under `~/.symposium/` by default. If [XDG Base Directory](https://specifications.freedesktop.org/basedir-spec/latest/) environment variables are set, `symposium` respects them:

| | Config | Cache | Logs |
|---|---|---|---|
| XDG set | `$XDG_CONFIG_HOME/symposium/` | `$XDG_CACHE_HOME/symposium/` | `$XDG_DATA_HOME/symposium/logs/` |
| Default | `~/.symposium/` | `~/.symposium/cache/` | `~/.symposium/logs/` |

## File locations

| Path | Purpose |
|------|---------|
| `<config-dir>/config.toml` | User configuration |
| `<cache-dir>/` | Cache directory (crate sources, plugin sources) |
| `<data-dir>/logs/` | Log files |
| `.symposium/config.toml` | Project configuration |
