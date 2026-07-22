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

[defaults]
symposium-recommendations = true
user-plugins = true

[[registry]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"

[[registry]]
name = "local-dev"
path = "my-plugins"
```

## Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-sync` | bool | `true` | Automatically run `cargo agents sync` during hook invocations. When enabled, skills are kept in sync with workspace dependencies without manual intervention. |
| `agents-syncing` | bool | `true` | Include each workspace plugin's `.agents/skills/` default skill group, so skills you author there install into every configured agent's skill directory (such as `.claude/skills/` or `.kiro/skills/`). Skills that symposium itself installed — identified by the `.symposium` marker file — are never treated as sources. See [Workspace skills](../workspace-skills.md) for the user-guide overview, or [Agents syncing](#agents-syncing-mirror-user-authored-skills) below for details. |
| `hook-scope` | string | `"global"` | Where agent hooks are installed. `"global"` writes to the user's home directory (e.g., `~/`). `"project"` writes to the project directory, keeping hooks local to the workspace. |
| `auto-update` | string | `"on"` | Controls automatic update behavior. `"off"` disables update checks entirely. `"warn"` checks the registry (at most once per 24 hours) and prints a message when a newer version is available. `"on"` automatically installs the update via `cargo install` and re-executes the command with the new binary. |

### Agents syncing: mirror user-authored skills

Agents such as Copilot, Gemini, Codex, Goose, and OpenCode all read skills from the vendor-neutral `.agents/skills/` directory, but Claude Code and Kiro use their own paths (`.claude/skills/` and `.kiro/skills/`). When `agents-syncing` is enabled, every [workspace plugin](../workspace-skills.md) — the workspace root and each member directory — carries a second default skill group, gated by the `workspace-member()` predicate:

```toml
[[skills]]
predicates = ["workspace-member()"]
source.path = ".agents/skills"
```

Skills you author in `.agents/skills/` therefore flow through the same pipeline as every other skill and install into each configured agent's own skill directory, so a single authored copy is visible to every agent. The `workspace-member()` gate is what keeps these maintainer skills from installing for *dependents* of a published crate — they apply only while working in the workspace itself.

Two `.symposium`-marker rules keep sources and copies distinct (symposium never writes a marker into a source, only into directories it installs):

- Skill discovery skips marker-bearing directories, so copies symposium installed into `.agents/skills/` (for agents that read it natively) are never re-discovered as sources.
- For an agent whose skill directory *is* `.agents/skills/`, a skill whose source already sits at its install slot is left in place — nothing is copied.

Installed copies receive the same marker and `*` `.gitignore` that plugin-installed skills get, which means: updates to the source are re-copied on each sync; removing the source removes the copies on the next sync (the normal stale-skill reap); disabling `agents-syncing = false` does the same; and a pre-existing user-managed directory in a target is never overwritten (the skill installs under a suffixed name instead).

Because these are real skills now, `SKILL.md` frontmatter must carry `name` and `description` like any other [skill definition](./skill-definition.md).

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

## `[telemetry]`

Opt-in, per-user usage telemetry. Off by default. When enabled, Symposium
appends anonymous events as JSON lines to a local, per-day log under
`~/.symposium/telemetry/`. **Nothing is uploaded automatically** — you inspect
and share the data yourself with `cargo agents telemetry show`. The preference
is also collected during `cargo agents init`. See the
[telemetry design chapter](../design/telemetry.md) for the event format.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Record anonymous usage events (session starts, prompts, tool usage — counts and metadata only, no prompt or command content). Toggle with `cargo agents telemetry enable` / `disable`. |

```toml
[telemetry]
enabled = true
```

## `[defaults]`

Controls the two built-in registries. Both are enabled by default.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `symposium-recommendations` | bool | `true` | Fetch plugins from the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository. |
| `user-plugins` | bool | `true` | Scan `~/.symposium/plugins/` for user-defined plugins. |

## `[[registry]]`

Defines additional registries — directories or repositories offering plugins. Each entry must have exactly one of `git` or `path`. `[[plugin-source]]` is the retired spelling of this table and is still accepted.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(required)* | A name for this registry. Used in logs and cache paths, and to attribute the plugins loaded from it. |
| `git` | string | — | Repository URL. Fetched and cached under `~/.symposium/cache/plugin-sources/`, then read as a local directory. |
| `path` | string | — | Local directory containing plugins. Relative paths are resolved from `~/.symposium/`. |
| `auto-update` | bool | `true` | Check for updates on startup. Only applies to `git` registries. |

## `[plugins]`

Enablement: which plugins are allowed to run at all, as distinct from the [predicates](./predicates.md) that decide *when* an enabled plugin applies.

Symposium trusts two things without asking: the workspace you are in, and the [registries](#registry) you configured — pointing config at a registry is the act of trusting its curation. Your dependency list is deliberately not on that list. Depending on a crate means compiling its code; it should not silently let the crate's author add instructions to your agent. So a plugin embedded in a dependency runs only once you say so, and a registry plugin that names no dependency anywhere is *dormant* — loaded and listed, but inactive — until you enable it by name.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-enable` | array of strings | `[]` | Dependency names whose embedded plugins load without being asked about. `"*"` pre-consents to every dependency. |
| `use` | array | `[]` | Plugins enabled deliberately. Each entry is either a plain name (enabled in every workspace) or `{ name = "...", workspace = "/path" }` (enabled only while working in that workspace root). |
| `disable` | array of strings | `[]` | Names that must never be enabled. Takes precedence over `auto-enable`, including over `"*"`. |

Names are matched hyphen/underscore-insensitively, like crate names: `widget-lib` and `widget_lib` are the same entry.

```toml
[plugins]
auto-enable = ["widget-lib"]
disable = ["noisy-crate"]
use = ["standalone-plugin", { name = "team-tools", workspace = "/home/me/work/service" }]
```

`use` is what wakes a dormant plugin, and it also enables a plugin whether or not any dependency references it. `auto-enable` is narrower: it is consent for what a dependency you already have carries with it.

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
| `~/.symposium/telemetry/` | Telemetry event log, one JSONL file per day (created when `[telemetry] enabled = true` and events are recorded) |
| `~/.symposium/plugins/` | User-defined plugins |
| `~/.symposium/cache/` | Cache directory (crate sources, plugin sources) |
| `~/.symposium/logs/` | Log files (one per invocation, timestamped) |
