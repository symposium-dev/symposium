# Configuration

Symposium works out of the box with no configuration. This page covers the things you might want to customize.

Symposium reads its configuration from `~/.symposium/config.toml`. The file is optional — all fields have defaults.

## Full example

```toml
cache_dir = "/custom/cache/path"

[logging]
level = "info"

[defaults]
symposium-recommendations = true
user-plugins = true

[hooks]
nudge-interval = 50  # prompts between re-nudges (0 = disable nudges)

[[plugin-source]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"
auto-update = false

[[plugin-source]]
name = "local-dev"
path = "my-plugins"
```

## Plugin sources

Symposium discovers skills and hooks from **plugin sources**. Two built-in sources are enabled by default:

1. **`symposium-recommendations`** — curated plugins from the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository.
2. **`user-plugins`** — your own plugins in `~/.symposium/plugins/`.

You can add more with `[[plugin-source]]` entries and disable either built-in with `[defaults]`.

### Managing plugins

```bash
symposium plugin list              # list all sources and their plugins
symposium plugin show my-plugin    # show a plugin's details
symposium plugin sync              # update all git-based sources
symposium plugin sync my-org       # update a specific source
```

## Config file reference

### Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cache_dir` | string | (see directory resolution) | Directory for cached data. Overrides environment-based resolution. |

### `[logging]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `level` | string | `"info"` | Minimum log level. One of: `trace`, `debug`, `info`, `warn`, `error`. |

Each invocation writes a log file to `~/.symposium/logs/`.

### `[defaults]`

Controls the two built-in plugin sources. Both are enabled by default.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `symposium-recommendations` | bool | `true` | Fetch plugins from the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository. |
| `user-plugins` | bool | `true` | Scan `~/.symposium/plugins/` for user-defined plugins. |

### `[hooks]`

Controls hook behavior (nudging, etc.).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `nudge-interval` | integer | `50` | Number of prompts before re-nudging about an unloaded crate skill. Set to `0` to disable nudges entirely. |

### `[[plugin-source]]`

Defines additional plugin sources. Each entry must have exactly one of `git` or `path`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | A name for this source (used in logs and cache paths). |
| `git` | string | — | GitHub repository URL. Fetched as a tarball and cached under `~/.symposium/cache/plugin-sources/`. |
| `path` | string | — | Local directory containing plugins. Relative paths are resolved from `~/.symposium/`. |
| `auto-update` | bool | `true` | Whether to check for updates on startup. Only applies to `git` sources. When `false`, the source is only fetched by `symposium plugin sync`. |

## Directory resolution

By default, all Symposium data lives under `~/.symposium/`. This can be overridden via environment variables:

| Priority | Config | Cache | Logs |
|----------|--------|-------|------|
| `SYMPOSIUM_HOME` | `$SYMPOSIUM_HOME/` | `$SYMPOSIUM_HOME/cache/` | `$SYMPOSIUM_HOME/logs/` |
| XDG | `$XDG_CONFIG_HOME/symposium/` | `$XDG_CACHE_HOME/symposium/` | `$XDG_DATA_HOME/symposium/logs/` |
| Default | `~/.symposium/` | `~/.symposium/cache/` | `~/.symposium/logs/` |

`SYMPOSIUM_HOME` takes precedence over XDG variables.

## Default file locations

| Path | Purpose |
|------|---------|
| `~/.symposium/config.toml` | User configuration |
| `~/.symposium/plugins/` | User-defined plugins |
| `~/.symposium/cache/` | Cache directory (crate sources, plugin sources, etc.) |
| `~/.symposium/logs/` | Log files |
