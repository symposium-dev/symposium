# Plugin definitions

A **symposium plugin** collects together all the extensions offered for a particular crate. Plugins are directories containing a `SYMPOSIUM.toml` manifest file that references skills, hooks, MCP servers, and other resources relevant to your crate. These extensions can be packaged within the plugin directory or the plugin can contain pointers to external repositories.

Plugins enable capabilities beyond standalone skills — they're needed when you want to add hooks or MCP servers. For simple skill publishing, see [Authoring a plugin](../crate-authors/authoring-a-plugin.md) instead.

## Example: a plugin definition with inline skills

You could define a plugin definition with inline skills by having a directory struct like this:

```
my-crate/
  Cargo.toml
  SYMPOSIUM.toml
  skills/
    skill-a/
      SKILL.md
    skill-b/
      SKILL.md
```

where `SYMPOSIUM.toml` is as follows:

```toml
[[skills]]
source.path = "skills"
```

That's it — `name` defaults to the crate name and `crates` defaults to `["*"]` (unconditional activation).

## Top-level fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | no | Plugin name. Defaults to the crate name + relative path to the directory containing `SYMPOSIUM.toml` (just the crate name if at root, e.g., `foo/bar` for `bar/SYMPOSIUM.toml` in crate `foo`). |
| `crates` | string or array | no | Which crates this plugin applies to. Defaults to `["*"]` (unconditional). See [Plugin-level filtering](#plugin-level-filtering). |
| `predicates` | array of strings | no | Predicates (`crate`, `shell`, `path_exists`, `env`, `not`, `any`, `all`) that must all hold for the plugin to apply. See [Predicates](./predicates.md). |
| `installations` | array of tables | no | Named installation declarations (`[[installations]]`). Hooks reference these by name. See [Installations](#installations). |
| `skills` | array of tables | no | Skill groups (`[[skills]]`). |
| `hooks` | array of tables | no | Hooks (`[[hooks]]`). |
| `predicate` | array of tables | no | Custom predicate definitions (`[[predicate]]`). See [Custom predicates](#predicate). |
| `mcp_servers` | array of tables | no | MCP server registrations (`[[mcp_servers]]`). |
| `plugins` | array of tables | default `source.path = "."` | Transitive plugin source declarations (`[[plugins]] source.*`). See [Plugin Sources](#plugins-source). |
| `discovery.allow` / `discovery.deny` | table or `"*"` | no | Candidate plugin sources approved or rejected for auto-discovery. See [Discovery policy](#discovery-policy). |
| `defaults.skills` | bool | `true` | Include implicit `skills/` (recursive, unconditional) and `.agents/skills/` (recursive, `workspace()` gated) skill sources. Set to `false` to suppress. |
| `defaults.plugins` | bool | `true` | Search subtree for nested `SYMPOSIUM.toml` files. Each nested manifest becomes its own independent plugin. Set to `false` to suppress. |

## Plugin-level filtering

The top-level `crates` field controls when the entire plugin is active:

```toml
name = "my-plugin"
crates = ["serde", "tokio"]  # Only active in projects using serde OR tokio

# OR use wildcard to always apply (this is the default if omitted)
crates = ["*"]
```

Plugin-level filtering is combined with skill group filtering using AND logic — both must match for skills to be available.

## `[[skills]]` groups

Each `[[skills]]` entry declares a group of skills.

| Field | Type | Description |
|-------|------|-------------|
| `crates` | string or array | Which crates this group advises on. Accepts a single string (`"serde"`) or array (`["serde", "tokio>=1.0"]`). See [Crate predicates](./crate-predicates.md) for syntax. |
| `predicates` | array of strings | Predicates (`crate`, `shell`, `path_exists`, `env`, `not`, `any`, `all`) that must all hold for the group to install. See [Predicates](./predicates.md). |
| `source.path` | string | Local directory containing skill subdirectories. Resolved relative to the manifest file. |
| `source.git` | string | GitHub URL pointing to a directory in a repository (e.g., `https://github.com/org/repo/tree/main/skills`). Symposium downloads the tarball, extracts the subdirectory, and caches it. |

A skill group must have exactly one of `source.path` or `source.git`.

### Semantics of `crates` predicates at multiple levels

When you apply the `crates` predicate at multiple levels, all levels must match. This can be used to narrow the set of crates that have skills versus the set that activates your plugin overall.

```toml
name = "my-plugin"

# Any of these crates activates the plugin
crates = ["foo", "bar", "baz"]

[[skills]]
crates = ["foo", "bar"]  # this group only applies when foo or bar is present
source.path = "skills/foo-bar"

[[skills]]
crates = ["baz"]         # this group only applies when baz is present
source.path = "skills/baz"
```

## Implicit installations from crate binary targets

Every binary target defined in the plugin crate's `Cargo.toml` is automatically available as an installation — referenced by name in `command` fields, with no explicit `[[installations]]` section needed. Additionally, a special `crate` installation refers to the crate's default binary target (if one exists). Local plugin crates are installed through Symposium's managed cargo cache before the binary runs.

This means a crate that ships a CLI tool can wire it into hooks with zero boilerplate:

```toml
[[hooks]]
event = "PreToolUse"
command = "my-tool"   # references the `my-tool` binary target directly
```

Symposium reads the crate's Cargo metadata to discover binary targets when resolving the `SYMPOSIUM.toml`.

## Installations

An **installation** describes how to obtain (and optionally pre-configure) something a hook will run. Hooks then reference an installation as their `command` — either by name (`command = "rtk"`) or inline at the use site (`command = { script = "scripts/x.sh" }`).

A `[[installations]]` entry has a `name` plus any of:

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | Optional. How to acquire bits onto disk. One of `cargo`, `github`, `binary` (see below). When omitted, no acquisition step runs. |
| `install_commands` | array of strings | Optional. Shell commands run (in order) after the source step. Useful for post-install setup such as aliasing, or when *only* have manual commands. Each command must exit zero. |
| `requirements` | array | Optional. Other installations to acquire whenever this one is referenced. Strings name `[[installations]]` entries; tables are inline declarations. |
| `executable` | string | Optional. Path to a binary to run. For `cargo`, the binary name (looked up in the install's `bin/` dir). For `github` / `binary`, a path inside the acquired tree. With no source, a path on disk. |
| `script` | string | Optional. Same resolution rules as `executable`, but invoked as `sh <path> <args>`. |
| `args` | array of strings | Optional. Default invocation arguments. |


`executable` and `script` are mutually exclusive — pick one. The hook layer applies the same rule, and **at most one of `executable` / `script` may be set across the hook AND the installation it references**. An installation may have neither (then it's pure setup — useful as a `requirements` entry). For a hook to run, the chosen layer pair must end up with exactly one runnable.

> Inline installations (used as `command` or as a requirement entry) accept the same fields, including `requirements`.

### Installation sources

#### `cargo`

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
version = "13.0.0"     # optional; defaults to latest stable
executable = "rg"      # the binary to run; if omitted and the crate has a single binary, that one is used
args = ["--version"]   # optional default args
```

Symposium attempts `cargo binstall` first, falls back to `cargo install`, and caches the result under `~/.symposium/cache/binaries/<crate>/<version>/bin/` (passing `--root` so the install doesn't pollute `~/.cargo/bin`). The chosen `executable` resolves to `<cache>/bin/<executable>`. Hooks that depend on this installation get `<cache>/bin/` prepended to `$PATH`, so scripts can invoke the binary by name.

To install from a git repo instead of crates.io, set `git`:

```toml
[[installations]]
name = "tool"
source = "cargo"
crate = "tool"
git = "https://github.com/example/tool"
executable = "tool"   # required for git sources (crates.io is not consulted)
```

To install into the user's global cargo location (`~/.cargo/bin`) instead of a symposium-managed cache, set `global = true`. No `--root` is passed; `$PATH` is not augmented (the binary is expected to already be on `$PATH`). This can be useful if you are using scripts which require globally-installed programs, or if you want to use tools separately in a CLI.

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
executable = "rg"
global = true
```

#### `github`

```toml
[[installations]]
name = "rtk-hooks"
source = "github"
url = "https://github.com/example/rtk-hooks"
script = "hooks/claude/rtk-rewrite.sh"   # optional; see below
args = ["--format"]
```

Acquires the repo (or a subtree, if `url` points at `…/tree/<ref>/<path>`) into a local cache. The chosen `executable` / `script` resolves to a file inside the cached tree.

`executable`/`script` may be set on the installation or on the hook (but not both, in any combination). Setting it on the installation pins this entry to a specific file; omitting it lets multiple hooks each pick a different file.

#### no source

Omit `source` entirely when you just need to point at a path on disk (or rely on `install_commands` to put one there):

```toml
[[installations]]
name = "tool"
executable = "/usr/local/bin/tool"
```

Or "shell-only" installations — useful as side-effect requirements:

```toml
[[installations]]
name = "setup"
install_commands = [
    "ln -sf $HOME/.cache/foo $HOME/.local/bin/foo",
]
```

## `[[hooks]]`

Each `[[hooks]]` entry declares a hook that responds to agent events. For the JSON schemas that symposium-format hooks receive and produce, see [Symposium hook events](./hook-events.md).

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match (e.g., `PreToolUse`). |
| `matcher` | string (optional) | Which tool invocations to match (e.g., `Bash`). Omit to match all. |
| `command` | string or table | What to run. A string names a `[[installations]]` entry (or an implicit binary target); a table is an inline installation (promoted to a synthetic entry named after the hook). |
| `executable` | string (optional) | Path to a binary inside (or relative to) the installation. At most one of `executable`/`script` set across hook + installation. |
| `script` | string (optional) | Path to a shell script to run via `sh`. Same exclusivity rule as `executable`. |
| `args` | array (optional) | Invocation arguments. Forbidden when the installation also declares `args`. |
| `requirements` | array (optional) | Installations to acquire before running. Same shape as `command` (string name or inline declaration). |
| `agent` | string (optional) | Restrict the hook to a specific agent (`claude`, `copilot`, `gemini`, `kiro`, …). |
| `format` | string | Wire format the handler expects on stdin. `symposium` (default): symposium converts the agent's event to its canonical format before delivering. Any agent name (`claude`, `codex`, `copilot`, `gemini`, `kiro`): the handler receives that agent's native wire format. Symposium always intermediates — it never registers plugin hooks directly into agent configs. See [Hooks](../crate-authors/authoring-a-plugin.md#hooks). |
| `predicates` | array (optional) | Predicates (`crate`, `shell`, `path_exists`, `env`, `not`, `any`, `all`) that must all hold for the hook to dispatch. Evaluated per-dispatch. See [Predicates](./predicates.md). |

### Examples

Run a cargo-installed binary as the hook:

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
executable = "rg"

[[hooks]]
name = "rg-version"
event = "PreToolUse"
command = "rg"
args = ["--version"]
```

Use an implicit binary target (no installation block needed):

```toml
# If the plugin crate has a binary target named "my-checker"
[[hooks]]
name = "check"
event = "PreToolUse"
command = "my-checker"
```

Install rtk as a side requirement and run a hook script from a separate github source:

```toml
[[installations]]
name = "rtk"
source = "cargo"
crate = "rtk"

[[installations]]
name = "rtk-hooks"
source = "github"
url = "https://github.com/example/rtk-hooks"

[[hooks]]
name = "rewrite"
event = "PreToolUse"
requirements = ["rtk"]
command = "rtk-hooks"
script = "hooks/claude/rtk-rewrite.sh"
args = ["--format"]
```

Inline a one-off cargo install directly:

```toml
[[hooks]]
name = "rg-test"
event = "PreToolUse"
command = { source = "cargo", crate = "ripgrep", executable = "rg" }
args = ["--version"]
```

Run a script file on disk (no source):

```toml
[[hooks]]
name = "check"
event = "PreToolUse"
command = { script = "scripts/check.sh", args = ["--strict"] }
```

A cargo install with a post-install step (e.g. to symlink a wrapper script):

```toml
[[installations]]
name = "rtk"
source = "cargo"
crate = "rtk"
install_commands = [
    "ln -sf $HOME/.symposium/cache/binaries/rtk/*/bin/rtk $HOME/.local/bin/rtk",
]

[[hooks]]
name = "rtk-rewrite"
event = "PreToolUse"
command = "rtk"
args = ["rewrite"]
```

### Agent-specific hooks

An agent-specific hook expects a particular agent's native wire format on stdin. Use this when you need full access to an agent's event schema. Symposium still intermediates — it delivers the input in the declared format (passing through unmodified when the current agent matches, or converting when it doesn't).

A plugin with a Claude-specific hook and a symposium fallback:

```toml
[[hooks]]
name = "check-claude"
event = "PreToolUse"
format = "claude"
command = "my-hook-binary"

[[hooks]]
name = "check-portable"
event = "PreToolUse"
format = "symposium"
command = "my-hook-binary"
args = ["--symposium"]
```

On Claude, `check-claude` fires (receives Claude's native JSON). On other agents, `check-portable` fires (receives symposium canonical JSON). Only one hook per plugin fires for a given event — symposium picks the best match by format priority.

### Requirements

`requirements` ensures other installations are acquired before the hook runs. Useful when the hook's command relies on something else being on disk (or eventually on `$PATH`).

```toml
[[hooks]]
name = "uses-rtk-via-script"
event = "PreToolUse"
requirements = ["rtk", { source = "cargo", crate = "ripgrep" }]
command = { script = "scripts/uses-rtk.sh" }
```

Requirements may also be declared on an `[[installations]]` entry. Whenever that installation is referenced — as a hook's `command` or in another `requirements` list — its declared requirements are appended (one level, prerequisites first):

```toml
[[installations]]
name = "rtk"
source = "cargo"
crate = "rtk"

[[installations]]
name = "rtk-hooks"
source = "github"
url = "https://github.com/example/rtk-hooks"
requirements = ["rtk"]   # rtk gets installed whenever rtk-hooks is used

[[hooks]]
name = "rewrite"
event = "PreToolUse"
command = "rtk-hooks"
script = "hooks/claude/rtk-rewrite.sh"
```

Requirement installation is best-effort: failures are logged and dispatch continues.

### Hook environment

Hooks are spawned with the following extras on top of the parent environment:

| Variable | When set | Value |
|----------|----------|-------|
| `$SYMPOSIUM_DIR_<name>` | Installation has a symposium-managed cache (scoped cargo, github) | Absolute path to the cache / clone directory. |
| `$SYMPOSIUM_<name>` | Installation resolves to a runnable with an absolute path | Absolute path to the resolved executable / script. |
| `$PATH` | One or more dependencies contribute a runnable with an absolute path | Each runnable's parent dir is prepended, with the hook's `command` first. |

`<name>` is the installation name with non-alphanumeric characters replaced by `_` (e.g. `rtk-hooks` → `SYMPOSIUM_DIR_rtk_hooks`). Both the hook's `command` installation and every requirement (recursively, one level via installation-level requirements) contribute.

Global cargo installs (`global = true`) don't set `$SYMPOSIUM_DIR_<name>` or augment `$PATH` — the binary is expected to already be on the user's `$PATH` via `~/.cargo/bin`.

> **`install_commands` runs before env vars are set.** The `$SYMPOSIUM_*` vars and the augmented `$PATH` are only available to the hook's spawned process. `install_commands` runs earlier, inside the symposium dispatch process, so it cannot reference its own (or any other) installation's env vars. Use absolute paths in `install_commands` instead.

### Supported hook events

| Hook event | Description | CLI usage |
|------------|-------------|-----------|
| `PreToolUse` | Before a tool (e.g., `Bash`) is invoked by the agent. | `pre-tool-use` |
| `PostToolUse` | After a tool completes. | `post-tool-use` |
| `UserPromptSubmit` | When the user submits a prompt. | `user-prompt-submit` |
| `SessionStart` | When an agent session starts. | `session-start` |

### Agent → hook name mapping

| Tool / Event | Claude (`claude`) | Copilot (`copilot`) | Gemini (`gemini`) |
|--------------|------------------------------------:|-------------------:|------------------:|
| `PreToolUse` | `PreToolUse` | `PreToolUse` | `BeforeTool` |

### Hook semantics

- **Exit codes**:
	- `0` — success: the hook's stdout is parsed as JSON and merged into the overall hook result.
	- `2` (or no reported exit code) — treated as a failure: dispatch stops immediately and the hook's stderr is returned to the caller.
	- any other non-zero code — treated as success for dispatching purposes; stdout is still parsed and merged when possible.

- **Stdout handling**: Hooks should write a JSON object to stdout to contribute structured data back to the caller. Valid JSON objects are merged together across successful hooks; keys from later hooks overwrite earlier keys.

- **Stderr handling**: If a hook exits with code `2` (or no exit code), dispatch returns immediately with the hook's stderr as the error message. Otherwise stderr is captured but not returned on success.

### Testing hooks

Use the CLI to test a hook with sample input:

```bash
echo '{"tool": "Bash", "input": "cargo test"}' | cargo agents hook claude pre-tool-use
```

You can also use `copilot`, `gemini`, `codex`, or `kiro` as the agent name.

## `[[plugins]] source.*`

Each `[[plugins]]` entry declares an additional plugin source to scan. Path sources are used for nested manifests inside the current source tree. Git and crate sources are resolved by their registries during source graph expansion.

| Field | Type | Description |
|-------|------|-------------|
| `source.path` | string | Directory to search recursively for `SYMPOSIUM.toml`, relative to the current manifest. |
| `source.git` | string | Direct git-registry plugin source URL. |
| `source.crate` | string or table | Crate-registry source using Cargo dependency syntax. |
| `where.crates` / `where.predicates` | arrays | Optional filters for the source declaration. |

### Examples

```toml
# Always also install a companion crate
[[plugins]]
source.crate.symposium-serde-extras = "1.0"

# Conditionally install based on workspace deps
[[plugins]]
where.crates = ["tokio"]
source.path = "tokio-extras"

[[plugins]]
where.crates = ["serde"]
source.crate.symposium-serde-extras = "1"

# Git source
[[plugins]]
source.git = "https://github.com/my-org/my-private-plugin"
```

Plugin sources are resolved recursively — if `symposium-serde-extras` itself has `[[plugins]]` entries, those are followed too.

A crate that doesn't want to bundle plugins directly just has a thin `SYMPOSIUM.toml` pointing elsewhere:

```toml
# A redirect-only plugin
[[plugins]]
source.crate.symposium-serde = "1.0"
```

## Discovery policy

Declares candidate plugin sources that are approved or rejected for automatic
plugin discovery. Primarily used by curated plugin crates like
`symposium-recommendations`:

```toml
[discovery.allow]
crates = { dial9 = "*", dial9-viewer = "*" }

[discovery.deny]
crates = { unsafe-plugin = "*" }
```

On sync, Symposium collects discovery rules from all installed plugins, checks
workspace deps and other candidate sources against them, and resolves matching
sources into plugin sources. See [Plugin sources](./plugin-source.md) for
details.

## `[[predicate]]`

Each `[[predicate]]` entry defines a custom predicate function that can be used in `predicates` expressions anywhere a predicate is accepted. Custom predicates extend the built-in predicate language with plugin-specific checks.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | The predicate name. Must be a valid identifier (`[a-zA-Z][a-zA-Z0-9_]*`) and must not collide with builtins (`crate`, `shell`, `path_exists`, `env`, `not`, `any`, `all`). |
| `command` | string or table | The installation to run. Same shape as hook `command` (a string naming a `[[installations]]` entry or an inline table). |
| `args` | array of strings | Optional. Static arguments passed to the command before the dynamic argument. |

### How custom predicates work

Custom predicates are registered globally — a predicate defined in one plugin can be used by any other plugin's `predicates` expressions. Registration is unconditional: even if the defining plugin's own crate predicates don't match the current workspace, its `[[predicate]]` entries are still available.

When a predicate expression uses a function name that isn't a builtin, Symposium looks it up in the custom predicate registry. If found, it spawns the declared command with the static `args` followed by the raw argument text from the expression.

```toml
[[installations]]
name = "cargo-bp-install"
source = "cargo"
crate = "cargo-bp"
executable = "cargo-bp"

[[predicate]]
name = "battery_pack"
command = "cargo-bp-install"
args = ["bp", "status", "--check"]
```

Usage in a `predicates` expression:

```toml
predicates = ["battery_pack(cli>=0.3)"]
```

This evaluates as:

```
cargo-bp bp status --check cli>=0.3
```

Exit 0 means the predicate passes; non-zero means it fails.

The argument is trimmed of leading/trailing whitespace before being passed. An empty argument — `battery_pack()` or `battery_pack( )` — does not append anything to the command (only the static `args` are passed).

### Collisions

If two plugins define the same predicate name, both definitions are skipped and a warning is emitted. Skills referencing the collided name evaluate as false.

### Caching

Results are cached by `(predicate_name, raw_arg)` for the duration of a single sync run. The same predicate called with the same argument is only spawned once.

## `[[mcp_servers]]`

Each `[[mcp_servers]]` entry declares an MCP server that Symposium registers into the agent's configuration during `sync --agent`.

There are multiple MCP transports:

### Stdio

```toml
[[mcp_servers]]
name = "my-server"
command = "/usr/local/bin/my-server"
args = ["--stdio"]
env = []
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Server name as it appears in the agent's MCP config. |
| `crates` | string or array | Which crates this server applies to. Optional if plugin has top-level `crates`. |
| `predicates` | array of strings | Predicates (`crate`, `shell`, `path_exists`, `env`, `not`, `any`, `all`) that must all hold for the server to register. See [Predicates](./predicates.md). |
| `command` | string | Path to the server binary. |
| `args` | array of strings | Arguments passed to the binary. |
| `env` | array of objects | Environment variables to set when launching the server. |

Stdio entries do not need a `type` field.

### HTTP

```toml
[[mcp_servers]]
type = "http"
name = "my-server"
url = "http://localhost:8080/mcp"
headers = []
```

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Must be `"http"`. |
| `name` | string | Server name as it appears in the agent's MCP config. |
| `crates` | string or array | Which crates this server applies to. Optional if plugin has top-level `crates`. |
| `url` | string | HTTP endpoint URL. |
| `headers` | array of objects | HTTP headers to set when making requests. |

### SSE

```toml
[[mcp_servers]]
type = "sse"
name = "my-server"
url = "http://localhost:8080/sse"
headers = []
```

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Must be `"sse"`. |
| `name` | string | Server name as it appears in the agent's MCP config. |
| `crates` | string or array | Which crates this server applies to. Optional if plugin has top-level `crates`. |
| `url` | string | SSE endpoint URL. |
| `headers` | array of objects | HTTP headers to set when making requests. |

### How registration works

During `cargo agents sync --agent`, each MCP server entry is written into the agent's config file in the format that agent expects. Registration is idempotent — existing entries with correct values are left untouched, stale entries are updated in place.

When a user runs `cargo agents sync` (or the hook triggers it automatically), Symposium:

1. Collects `[[mcp_servers]]` entries from all enabled plugins.
2. Writes each server into the agent's MCP configuration file.

All supported agents have MCP server configuration. Symposium handles the format differences — you declare the server once and it works across agents.

| Agent | Config location | Key |
|-------|----------------|-----|
| Claude Code | `.claude/settings.json` | `mcpServers.<name>` |
| GitHub Copilot | `.vscode/mcp.json` | `<name>` (top-level) |
| Gemini CLI | `.gemini/settings.json` | `mcpServers.<name>` |
| Codex CLI | `.codex/config.toml` | `[mcp_servers.<name>]` |
| Kiro | `.kiro/settings/mcp.json` | `mcpServers.<name>` |
| OpenCode | `opencode.json` | `mcp.<name>` |
| Goose | `~/.config/goose/config.yaml` | `extensions.<name>` |

## Example: full manifest

```toml
name = "widgetlib"
crates = ["widgetlib"]

# Skills alongside the plugin manifest
[[skills]]
crates = ["widgetlib"]
source.path = "skills"

# Additional skills hosted in a git repo
[[skills]]
crates = ["widgetlib=1.0"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/serde-skills"

[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "Bash"
command = "widgetlib-checker"  # implicit installation from crate binary target

[[mcp_servers]]
name = "widgetlib-mcp"
command = "/usr/local/bin/widgetlib-mcp"
args = ["--stdio"]
env = []

# When this plugin is installed, also scan the extras crate
[[plugins]]
source.crate.widgetlib-extras = "1.0"
```

## Validation

```bash
cargo agents plugin validate path/to/SYMPOSIUM.toml
```

This parses the manifest and reports any errors. Crate name checking against crates.io is on by default; use `--no-check-crates` to skip it.
