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

An **installation** describes how to obtain (and optionally pre-configure) something a hook will run. Hooks then reference an installation as their `command` — either by name (`command = "rtk"`) or inline at the use site (`command = { source = "cargo", … }`).

A `[[installations]]` entry has a `name` plus the source-specific fields below.

### Installation sources

Each installation has a `source` discriminator that determines how the bits get on disk:

- **Single binary**: `cargo`, `local`, `binary` — produces one executable.
- **Directory of files**: `github` — clones (a subtree of) a repo; the hook (or installation) picks a sub-path.
- **No file**: `shell` — a shell string run via `sh -c`.

`args` may appear on most installation variants as a default invocation; the hook command can override.

#### `cargo`

```toml
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
version = "13.0.0"   # optional; defaults to latest stable
binary = "rg"        # required if the crate exposes multiple binaries
args = ["--version"] # optional default args
```

Symposium attempts `cargo binstall` first, falls back to `cargo install`, and caches the result under `~/.symposium/cache/binaries/<crate>/<version>/bin/`. Hook-level `path` is not allowed (cargo always produces a single binary).

#### `local`

```toml
[[installations]]
name = "check"
source = "local"
command = "./scripts/check.sh"   # relative to plugin dir, or absolute
args = ["--strict"]              # optional default args
```

#### `github`

```toml
[[installations]]
name = "rtk-hooks"
source = "github"
url = "https://github.com/example/rtk-hooks"
path = "hooks/claude/rtk-rewrite.sh"   # optional; see below
args = ["--format"]                    # only valid when `path` is also set
```

Acquires the repo (or a subtree, if `url` points at `…/tree/<ref>/<path>`) into a local cache.

A path inside the repo must be supplied, **on the installation OR on the hook — not both**. Setting `path` on the installation pins this entry to a specific file; omitting it lets multiple hooks each pick a different file via hook-level `path`.

`args` on the installation is only valid when `path` is also set there (otherwise there's no executable yet to apply args to).

#### `binary`

Per-platform prebuilt archives:

```toml
[[installations]]
name = "agent-hook"
source = "binary"
"linux-x86_64"   = { archive = "https://example.com/linux.tar.gz",  cmd = "./hook" }
"darwin-aarch64" = { archive = "https://example.com/macos.tar.gz", cmd = "./hook" }
```

Symposium selects the entry matching the current platform (`darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`, `linux-aarch64`, `windows-x86_64`, etc.), downloads and extracts the archive once, and runs `cmd`.

#### `shell`

```toml
[[installations]]
name = "log-hi"
source = "shell"
command = "echo $1"
args = ["hello"]   # optional; passed as positional parameters $1, $2, …
```

Symposium spawns `sh -c <command> sh <args…>`, so user-supplied args are visible inside the shell command as `$1`, `$2`, … (`$0` is the literal `"sh"`).

## `[[hooks]]`

Each `[[hooks]]` entry declares a hook that responds to agent events.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match (e.g., `PreToolUse`). |
| `matcher` | string (optional) | Which tool invocations to match (e.g., `Bash`). Omit to match all. |
| `command` | string or table | What to run. A string names a `[[installations]]` entry; a table is an inline installation declaration (promoted to a synthetic `[[installations]]` entry named after the hook). |
| `path` | string (optional) | For github installations, the file inside the cached repo. Forbidden for other sources. Must not also be set on the installation. |
| `args` | array (optional) | Invocation arguments. Forbidden when the same installation also declares `args`. |
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
binary = "rg"

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
path = "hooks/claude/rtk-rewrite.sh"
args = ["--format"]
```

Inline a one-off cargo install directly:

```toml
[[hooks]]
name = "rg-test"
event = "PreToolUse"
command = { source = "cargo", crate = "ripgrep", binary = "rg" }
args = ["--version"]
```

Run a shell command:

```toml
[[hooks]]
name = "echo-hi"
event = "PreToolUse"
command = { source = "shell", command = "echo hi" }
```

Run a local script bundled with the plugin:

```toml
[[hooks]]
name = "check"
event = "PreToolUse"
command = { source = "local", command = "scripts/check.sh" }
args = ["--strict"]
```

### Requirements

`requirements` ensures other installations are acquired before the hook runs. Useful when the hook's command relies on something else being on disk (or eventually on `$PATH`).

```toml
[[hooks]]
name = "uses-rtk-via-script"
event = "PreToolUse"
requirements = ["rtk", { source = "cargo", crate = "ripgrep" }]
command = { source = "local", command = "scripts/uses-rtk.sh" }
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
command = "rtk-hooks"   # implicitly pulls in `rtk` as a requirement
path = "hooks/claude/rtk-rewrite.sh"
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
