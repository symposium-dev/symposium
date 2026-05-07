# Plugin definitions

A **symposium plugin** collects together all the extensions offered for a particular crate. Plugins are directories containing a `SYMPOSIUM.toml` manifest file that references skills, hooks, MCP servers, and other resources relevant to your crate. These extensions can be packaged within the plugin directory or the plugin can contain pointers to external repositories.

Plugins enable capabilities beyond standalone skills — they're needed when you want to add hooks or MCP servers. For simple skill publishing, see [standalone skills](../crate-authors/publishing-skills.md) instead.

## Example: a plugin definition with inline skills

You could define a plugin definition with inline skills by having a directory struct like this:

```
myplugin/
  SYMPOSIUM.toml
  skills/
    skill-a/
      SKILL.md
    skill-b/
      SKILL.md
```

where `myplugin/SYMPOSIUM.toml` is as follows:

```toml
name = "example"
crates = ["*"]

[[skills]]
source.path = "skills"
```

## Top-level fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Plugin name. Used in logs and CLI output. |
| `crates` | string or array | no | Which crates this plugin applies to. Use `["*"]` for all crates. See [Plugin-level filtering](#plugin-level-filtering). |
| `installations` | array of tables | no | Named installation declarations (`[[installations]]`). Hooks reference these by name. See [Installations](#installations). |
| `skills` | array of tables | no | Skill groups (`[[skills]]`). |
| `hooks` | array of tables | no | Hooks (`[[hooks]]`). |
| `mcp_servers` | array of tables | no | MCP server registrations (`[[mcp_servers]]`). |

**Note**: Every plugin must specify `crates` somewhere — either at the plugin level, in `[[skills]]` groups, or in `[[mcp_servers]]` entries. Plugins without any crate targeting will fail validation.

## Plugin-level filtering

The top-level `crates` field controls when the entire plugin is active:

```toml
name = "my-plugin"
crates = ["serde", "tokio"]  # Only active in projects using serde OR tokio

# OR use wildcard to always apply
crates = ["*"]
```

If omitted, the plugin applies to all projects. Plugin-level filtering is combined with skill group filtering using AND logic — both must match for skills to be available.

## `[[skills]]` groups

Each `[[skills]]` entry declares a group of skills.

| Field | Type | Description |
|-------|------|-------------|
| `crates` | string or array | Which crates this group advises on. Accepts a single string (`"serde"`) or array (`["serde", "tokio>=1.0"]`). See [Crate predicates](./crate-predicates.md) for syntax. |
| `source.path` | string | Local directory containing skill subdirectories. Resolved relative to the manifest file. |
| `source.git` | string | GitHub URL pointing to a directory in a repository (e.g., `https://github.com/org/repo/tree/main/skills`). Symposium downloads the tarball, extracts the subdirectory, and caches it. |

A skill group must have exactly one of `source.path` or `source.git`.

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

Symposium attempts `cargo binstall` first, falls back to `cargo install`, and caches the result under `~/.symposium/cache/binaries/<crate>/<version>/bin/`. The chosen `executable` resolves to `<cache>/bin/<executable>`.

To install from a git repo instead of crates.io, set `git`:

```toml
[[installations]]
name = "tool"
source = "cargo"
crate = "tool"
git = "https://github.com/example/tool"
executable = "tool"   # required for git sources (crates.io is not consulted)
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

Each `[[hooks]]` entry declares a hook that responds to agent events.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match (e.g., `PreToolUse`). |
| `matcher` | string (optional) | Which tool invocations to match (e.g., `Bash`). Omit to match all. |
| `command` | string or table | What to run. A string names a `[[installations]]` entry; a table is an inline installation (promoted to a synthetic entry named after the hook). |
| `executable` | string (optional) | Path to a binary inside (or relative to) the installation. At most one of `executable`/`script` set across hook + installation. |
| `script` | string (optional) | Path to a shell script to run via `sh`. Same exclusivity rule as `executable`. |
| `args` | array (optional) | Invocation arguments. Forbidden when the installation also declares `args`. |
| `requirements` | array (optional) | Installations to acquire before running. Same shape as `command` (string name or inline declaration). |
| `agent` | string (optional) | Restrict the hook to a specific agent (`claude`, `copilot`, `gemini`, `kiro`, …). |
| `format` | string | Wire format for hook input/output. One of: `symposium` (default), `claude`, `codex`, `copilot`, `gemini`, `kiro`. |

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

[[skills]]
crates = ["widgetlib=1.0"]
source.path = "skills/general"

[[skills]]
crates = ["widgetlib=1.0"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/serde-skills"

[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "Bash"
command = { source = "local", command = "./scripts/check-widget.sh" }

[[mcp_servers]]
name = "widgetlib-mcp"
command = "/usr/local/bin/widgetlib-mcp"
args = ["--stdio"]
env = []
```

## Validation

```bash
cargo agents plugin validate path/to/symposium.toml
```

This parses the manifest and reports any errors. Crate name checking against crates.io is on by default; use `--no-check-crates` to skip it.
