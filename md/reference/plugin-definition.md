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

## `[[hooks]]`

Each `[[hooks]]` entry declares a hook that responds to agent events.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Descriptive name for the hook (used in logs). |
| `event` | string | Event type to match (e.g., `PreToolUse`). |
| `matcher` | string | Which tool invocations to match (e.g., `Bash`). Omit to match all. |
| `command` | string (optional) | Command to run when the hook fires. Resolved relative to the plugin directory. If `distribution` is provided, `command` should not be specified and is ignored. |
| `distribution` | object (optional) | Describe how to obtain and run the hook as a distributable artifact. |
| `requirements` | array of objects (optional) | A list of installation sources that must be present before running the hook. |
| `agent` | string (optional) | Restrict the hook to a specific agent (e.g., `claude`, `copilot`, `gemini`, `kiro`). |
| `format` | string | Wire format for hook input/output. One of: `symposium` (default), `claude`, `codex`, `copilot`, `gemini`, `kiro`. Controls how the hook receives input and returns output. |

### Hook distributions

Hooks may be provided as a local `command` (the historical behavior) or as a `distribution` object. The `distribution` object must include a `source` discriminator and the fields listed for that source below.

#### `cargo`

- TOML example:

```toml
distribution = { source = "cargo", crate = "ripgrep", version = "13.0.0", binary = "rg", args = ["--version"] }
```

- Fields:
  - `source` (string, required) — must be `"cargo"`.
  - `crate` (string, required) — crate name on crates.io.
  - `version` (string, optional) — semver or exact version; if omitted the latest stable is used.
  - `binary` (string, optional) — explicit binary name to run; if omitted the crate must expose a single binary.
  - `args` (array of strings, optional) — args passed to the binary.
  - `path` (string, optional) — use `cargo install --path <path>` when present.
  - `git` (string, optional) — use `cargo install --git <git>` when present.

- Semantics: when `path` or `git` is present Symposium installs from source using cargo. Otherwise it resolves the version (querying crates.io when needed), attempts to install a prebuilt binary (via `cargo binstall` if available) and falls back to `cargo install`. Installed binaries are cached under Symposium's binary cache (e.g., `binaries/<crate>/<version>/bin/`). The hook executes the installed binary with any configured `args`.

#### `local`

- TOML example:

```toml
distribution = { source = "local", command = "./scripts/check-widget.sh", args = ["--flag"] }
```

- Fields:
  - `source` (string, required) — must be `"local"`.
  - `command` (string, required) — path to executable (resolved relative to the plugin directory, or absolute).
  - `args` (array of strings, optional)

- Semantics: the command is executed directly from the resolved path with the specified `args`.

#### `github`

- TOML example:

```toml
distribution = { source = "github", url = "https://github.com/org/repo", path = "bin/agent-hook", args = ["--help"] }
```

- Fields:
  - `source` (string, required) — must be `"github"`.
  - `url` (string, required) — GitHub repository or tree URL identifying the source repo.
  - `path` (string, required) — path inside the repo to the executable file.
  - `args` (array of strings, optional)

- Semantics: Symposium fetches and caches the repository (or the referenced subtree), verifies the `path` exists in the cache, and runs the file at the cached path with any `args`.

#### `binary`

- TOML example (per-platform inline tables; keys that include `-` may be quoted):

```toml
distribution = {
  source = "binary",
  ["linux-x86_64"] = { archive = "https://example.com/linux.tar.gz", cmd = "./hook", args = ["--serve"] },
  ["darwin-aarch64"] = { archive = "https://example.com/macos.tar.gz", cmd = "./hook" }
}
```

- Fields (per-platform table):
  - key: platform string (e.g., `linux-x86_64`, `darwin-aarch64`, `windows-x86_64`).
  - `archive` (string, required) — URL to an archive containing the binary (tar.gz, zip).
  - `cmd` (string, required) — path within the archive to the executable to run (may include `./`).
  - `args` (array of strings, optional)

- Semantics: Symposium selects the entry matching the current platform (see platform key mapping), downloads and caches the archive, extracts it into the cache, ensures executable permissions, and runs `cmd` with `args`.

Note: if both a top-level `command` and a `distribution` are present on a hook, Symposium prefers `distribution` and ignores `command` (a warning is logged).

### Requirements (installation sources)

Hooks may declare a `requirements` array. Each entry describes an installation source Symposium will attempt to satisfy before executing the hook. Install attempts are best-effort: Symposium logs installation errors and continues with hook dispatch rather than failing the entire event.

#### `cargo`

- TOML example:

```toml
requirements = [
  { type = "cargo", crate = "my-tool", version = "0.2.1", binary = "mytool" },
  { type = "cargo", crate = "other", git = "https://github.com/org/other" }
]
```

- Fields:
  - `type` (string, required) — must be `"cargo"`.
  - `crate` (string, required) — crate name on crates.io.
  - `version` (string, optional) — version to install; if omitted the latest stable is used.
  - `binary` (string, optional) — expected binary name (used for cache lookup / verification).
  - `path` (string, optional) — if present, install using `cargo install --path <path>`.
  - `git` (string, optional) — if present, install using `cargo install --git <git>`.

- Semantics: for registry installs (no `git`/`path`) Symposium resolves the version (when needed), tries to obtain a prebuilt binary (via `cargo binstall` if available) and falls back to `cargo install`. For `git` or `path` installs it uses `cargo install` with the matching flag. Installed artifacts are cached under Symposium's binary cache. Installation failures are logged and do not abort hook dispatch.


### Supported hook events

| Hook event | Description | CLI usage |
|------------|-------------|-----------|
| `PreToolUse` | Triggered before a tool (for example, `Bash`) is invoked by the agent. | `pre-tool-use` |

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
command = "./scripts/check-widget.sh"

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
