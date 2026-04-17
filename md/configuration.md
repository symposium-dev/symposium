# Configuration

Symposium reads its configuration from `~/.symposium/config.toml`. The file is optional — Symposium uses defaults when it is absent or when individual fields are omitted.

## Directory resolution

By default, all Symposium data lives under `~/.symposium/`. This can be overridden via environment variables:

| Priority | Config | Cache | Logs |
|----------|--------|-------|------|
| `SYMPOSIUM_HOME` | `$SYMPOSIUM_HOME/` | `$SYMPOSIUM_HOME/cache/` | `$SYMPOSIUM_HOME/logs/` |
| XDG | `$XDG_CONFIG_HOME/symposium/` | `$XDG_CACHE_HOME/symposium/` | `$XDG_DATA_HOME/symposium/logs/` |
| Default | `~/.symposium/` | `~/.symposium/cache/` | `~/.symposium/logs/` |

`SYMPOSIUM_HOME` takes precedence over XDG variables. To remove all Symposium data, delete the directory (by default `rm -rf ~/.symposium`).

## Default file locations

| Path | Purpose |
|------|---------|
| `~/.symposium/config.toml` | User configuration |
| `~/.symposium/plugins/` | User-defined plugins |
| `~/.symposium/cache/` | Cache directory (crate sources, plugin sources, etc.) |
| `~/.symposium/logs/` | Log files (one per invocation, timestamped) |

Directories are created automatically on first use.

## Reference

```toml
auto-sync = true

[[agent]]
name = "claude"

[[agent]]
name = "gemini"

cache_dir = "/custom/cache/path"  # optional, overrides cache location

[logging]
level = "info"  # trace, debug, info, warn, error

[defaults]
symposium-recommendations = true  # fetch plugins from the symposium-dev/recommendations repo
user-plugins = true               # scan ~/.symposium/plugins/ for local plugins

[[plugin-source]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"
auto-update = false

[[plugin-source]]
name = "local-dev"
path = "my-plugins"  # relative to ~/.symposium/
```

### Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-sync` | bool | `true` | Automatically run `symposium sync` during hook invocations to keep skills in sync with workspace dependencies. |
| `cache_dir` | string | (see directory resolution) | Directory for cached data (extracted crate sources, etc.). Overrides the environment-based resolution. |

### `[[agent]]`

Each `[[agent]]` entry identifies an agent you use. You can configure multiple agents.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | Agent name: `claude`, `codex`, `copilot`, `gemini`, `goose`, `kiro`, or `opencode`. |

### `[logging]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `level` | string | `"info"` | Minimum log level. One of: `trace`, `debug`, `info`, `warn`, `error`. |

### `[defaults]`

Controls the two built-in plugin sources. Both are enabled by default.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `symposium-recommendations` | bool | `true` | Fetch plugins from the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository. |
| `user-plugins` | bool | `true` | Scan `~/.symposium/plugins/` for user-defined plugins. |

### `[[plugin-source]]`

Defines additional plugin sources. Each entry must have exactly one of `git` or `path`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | A name for this source (used in logs and cache paths). |
| `git` | string | — | GitHub repository URL. The repo is fetched as a tarball and cached under `~/.symposium/cache/plugin-sources/`. |
| `path` | string | — | Local directory containing plugins. Relative paths are resolved from `~/.symposium/`. |
| `auto-update` | bool | `true` | Whether to check for updates on startup. Only applies to `git` sources. When `false`, the source is only fetched by `symposium plugin sync`. |

## Plugin sources

Symposium discovers plugins from one or more **plugin sources**. A plugin source is either a GitHub repository or a local directory containing plugin TOML files.

Two built-in sources are enabled by default:

1. **`symposium-recommendations`** — the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository. Fetched as a tarball on first run, cached locally, and checked for updates on each startup.
2. **`user-plugins`** — the `~/.symposium/plugins/` directory. Place your own `.toml` plugin files here.

You can add more sources with `[[plugin-source]]` entries and disable either built-in with `[defaults]`.

### Updating

Git-based sources with `auto-update = true` (the default) are checked for freshness on startup. If the upstream commit has changed, the cached copy is refreshed. Network failures fall back to the existing cache.

### Plugin CLI commands

The `symposium plugin` subcommand provides plugin management:

```bash
# Sync all git-based plugin sources (respects auto-update)
symposium plugin sync

# Sync a specific provider (ignores auto-update)
symposium plugin sync my-org

# List all providers and their plugins
symposium plugin list

# Show a plugin's TOML configuration and source file path
symposium plugin show my-plugin
```

## Logging

Each invocation of `symposium` writes a log file to the logs directory with a timestamped filename (e.g., `symposium-20260325-154226.log`).

To see hook payloads and other verbose output, set the log level to `debug`:

```toml
[logging]
level = "debug"
```
