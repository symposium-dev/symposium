# Current status

Symposium is in early development. This page describes what works today. For the full vision, see the [design overview](./overview.md).

## What works

### Active workflow

The core workflow is:

1. A **static skill** tells the agent to run `symposium start` before writing Rust
2. `symposium start` returns **general Rust guidance + a dynamic crate skill list** for the workspace
3. **PostToolUse hooks** record when the agent loads crate guidance (via `symposium crate <name>` or the MCP tool)
4. **UserPromptSubmit hooks** scan prompts for crate names in code-like contexts and nudge the agent about available skills it hasn't loaded

### Hooks

`symposium hook <event>` handles hook events from editor plugins. Three hook events are supported:

- **PreToolUse** — dispatches plugin hooks (spawn commands). No built-in logic.
- **PostToolUse** — records skill activations when the agent successfully invokes `symposium crate <name>` (Bash) or the MCP `rust` tool with `["crate", "<name>"]`. Also detects when the agent reads a known skill directory path.
- **UserPromptSubmit** — scans the prompt for crate names in code-like contexts (backticks, fenced code blocks, Rust paths). Nudges the agent about available skills it hasn't loaded, with configurable re-nudge interval.

### SQLite state tracking

Hook invocations share state via a SQLite database at `~/.symposium/state.0.sqlite` (schema version in filename). Tracks:

- **SkillActivation** — which crate skills have been loaded in each session
- **SkillNudge** — nudge history to avoid repeating
- **SessionState** — per-session prompt count
- **WorkspaceCache** — cached workspace deps keyed by `Cargo.lock` mtime
- **AvailableSkill** — skills available for the workspace, populated at hook entry

### Unified dispatch

Both CLI and MCP route through a shared dispatch layer (`src/dispatch.rs`). The MCP server exposes a single `rust` tool taking `args: Vec<String>`:

- `["start"]` — Rust guidance + dynamic crate skill list
- `["crate", "--list"]` — list workspace crates with available skills
- `["crate", "<name>"]` — crate info + guidance
- `["help"]` — help text

### Configuration

`~/.symposium/config.toml` provides user configuration:

```toml
[logging]
level = "info"  # trace, debug, info, warn, error

[defaults]
symposium-recommendations = true  # built-in plugin source (default: true)
user-plugins = true               # ~/.symposium/plugins/ (default: true)

[hooks]
nudge-interval = 50  # prompts between re-nudges (0 = disable nudges)

[[plugin-source]]
name = "my-org"
git = "https://github.com/my-org/symposium-plugins"
auto-update = false  # default: true

[[plugin-source]]
name = "local-dev"
path = "my-plugins"  # relative to ~/.symposium/
```

### Logging

All symposium invocations emit structured logs to `~/.symposium/logs/`. The log level is configured via `config.toml`.

### MCP server

`symposium mcp` runs an MCP server over stdio, exposing the unified `rust` tool. The tool dispatches the same way as CLI subcommands.

### Plugin sources

Plugins are discovered from configured **plugin sources**. Two built-in sources are enabled by default:

1. **`symposium-recommendations`** — the [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) repository, fetched as a tarball and cached under `~/.symposium/cache/plugin-sources/`.
2. **`user-plugins`** — the `~/.symposium/plugins/` directory for user-defined plugins.

Additional sources can be added via `[[plugin-source]]` in `config.toml`. Sources can point at a GitHub URL (`git`) or a local path (`path`, relative to `~/.symposium/` or absolute). Git sources are checked for freshness on startup and auto-updated; `auto-update = false` disables this (use `symposium plugin sync` to refresh manually).

Either built-in source can be disabled via `[defaults]` in `config.toml`.

### Plugins

A plugin is a TOML file. It can be a standalone `.toml` file or a `symposium.toml` inside a directory. Either way, the TOML is the plugin.

A plugin declares one or more `[[skills]]` groups. Each group specifies which crates it advises on and where the skill files come from:

```toml
name = "widgetlib-serde"

# group of skills for serialization in widgetlib 1.0
[[skills]]
crates = ["widgetlib=1.0", "serde"]
source.git = "https://github.com/org/repo/tree/main/widgetlib-serde"
```

### Skills

A skill group points at a directory following this layout:

```
dir/
    skills/
        skill-name/
            SKILL.md
            scripts/         # optional
            resources/       # optional
        another-skill/
            SKILL.md
```

Each `SKILL.md` follows the [agentskills.io](https://agentskills.io/specification.md) format: YAML-style frontmatter (name, description, license, compatibility, allowed-tools) and a markdown body.

## How to use it

There are three ways to use Symposium today:

### Claude Code plugin

Install the plugin to get a static skill (tells the agent to run `symposium start`) and automatic hook integration (PreToolUse, PostToolUse, UserPromptSubmit).

```bash
claude --plugin-dir path/to/agent-plugins/claude-code
```

See [How to install](../install.md) for details.

### MCP server

Configure your editor or agent to run `symposium mcp` as an MCP server over stdio.

### Direct CLI

If Symposium is on your PATH:

```bash
symposium start              # Rust guidance + crate skill list
symposium crate tokio        # crate-specific guidance
symposium crate --list       # list available crate skills
symposium hook pre-tool-use  # reads event JSON from stdin
```

## What's not yet implemented

The [design overview](./overview.md) describes the full architecture. The following are planned but not yet built:

- **Token-optimized cargo** — Cargo output filtering for token efficiency (temporarily removed, returning in a future release)
- **ACP agent** — Full interception via the Agent Client Protocol
- **Editor extensions** — Native integrations for VSCode, Zed, and IntelliJ
- **`symposium update`** — Self-update of the symposium binary (plugin source updates are implemented)
