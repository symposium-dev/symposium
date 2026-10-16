# Codex CLI Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Codex CLI's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [Hooks](https://learn.chatgpt.com/docs/hooks)
> · [GitHub repo](https://github.com/openai/codex)

OpenAI's Codex CLI implements a shell-command hook system configured in `hooks.json`. It first shipped in v0.114 (March 2026), behind an opt-in `codex_hooks` flag; hooks are now on by default, and `[features] hooks = false` in `~/.codex/config.toml` turns them off.

## Trust

Codex runs a non-managed hook only after the user trusts that exact definition. Trust is recorded against the hook's hash, so a new or changed hook is skipped until reviewed: Codex prints a startup warning pointing at `/hooks`, where hooks are reviewed, trusted or disabled. Project hooks additionally load only when the project's `.codex/` layer is trusted. For automation that vets hooks itself, `codex exec --dangerously-bypass-hook-trust` runs enabled hooks for one invocation. Observed on Codex 0.147: until trusted, symposium's hooks simply do not run.

## Configuration

| File | Scope |
|---|---|
| `~/.codex/hooks.json` | User-global |
| `<repo>/.codex/hooks.json` | Project-scoped |

Both are **additive** — all matching hooks from all files run. Project hooks follow the untrusted-project trust model.

### Configuration structure

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "python3 ~/.codex/hooks/check_bash.py",
        "statusMessage": "Checking command safety",
        "timeout": 30
      }]
    }]
  }
}
```

Only handler type is `"command"`. `matcher` is a regex string; omit or use `""` / `"*"` to match everything. Default timeout: **600 seconds**, configurable via `timeout` or `timeoutSec`.

## Events

| Event | Trigger | Matcher filters on | Can block? |
|---|---|---|---|
| `SessionStart` | Session starts or resumes | `source` (`"startup"` or `"resume"`) | Yes (`continue: false`) |
| `PreToolUse` | Before tool execution | `tool_name` (`"Bash"`, `"apply_patch"`, MCP tools) | Yes |
| `PostToolUse` | After tool execution | `tool_name` (`"Bash"`, `"apply_patch"`, MCP tools) | Yes (`continue: false`) |
| `UserPromptSubmit` | User submits a prompt | N/A | Yes (`continue: false`) |
| `Stop` | Agent turn completes | N/A | Yes (deny → continuation prompt) |

## Input Schema (stdin)

### Base fields (all events)

```json
{
  "session_id": "string",
  "transcript_path": "string|null",
  "cwd": "string",
  "hook_event_name": "string",
  "model": "string"
}
```

Turn-scoped events (PreToolUse, PostToolUse, UserPromptSubmit, Stop) add `turn_id`.

### PreToolUse additions

- `tool_name`: string
- `tool_use_id`: string
- `tool_input`: object with `command` field

### PostToolUse additions

- `tool_name`, `tool_use_id`, `tool_input` (same as PreToolUse)
- `tool_response`: string

### UserPromptSubmit additions

- `prompt`: string

### Stop additions

- `stop_hook_active`: boolean
- `last_assistant_message`: string

## Output Schema (stdout)

### Deny/block (two equivalent methods)

Method 1 — JSON output:
```json
{ "decision": "block", "reason": "Destructive command blocked" }
```

or:

```json
{
  "hookSpecificOutput": {
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked"
  }
}
```

Method 2 — **Exit code 2** with reason on stderr.

### Inject context

```json
{
  "hookSpecificOutput": {
    "additionalContext": "Extra info for the agent"
  }
}
```

Plain text on stdout also works for SessionStart and UserPromptSubmit (ignored for PreToolUse, PostToolUse, Stop).

### Stop session

```json
{ "continue": false, "stopReason": "Session terminated by hook" }
```

Supported on SessionStart, UserPromptSubmit, PostToolUse, Stop.

### System message (UI warning)

```json
{ "systemMessage": "Warning text shown to user" }
```

### Stop event special behavior

For the Stop event, `{ "decision": "block", "reason": "Run tests again" }` tells Codex to create a **continuation prompt** — it does not reject the turn.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success; stdout parsed. No output = continue normally. |
| `2` | Block/deny; stderr used as reason |
| Other | Non-blocking warning |

## Execution Behavior

- Multiple matching hooks run **concurrently** — no ordering guarantees.
- Commands run with session `cwd` as working directory.
- Shell expansion works.

## Rewriting tool input

Per the current docs, `PreToolUse` can rewrite a call with `hookSpecificOutput.updatedInput`, but only together with `permissionDecision: "allow"`, which also approves the call: for `Bash` and `apply_patch` it is a string `command`, for MCP and other tools the replacement arguments object. There is no `"ask"` to leave the rewritten call to the user. Earlier releases accepted `updatedInput` without applying it.

## Current Limitations

- No async hook mode.
- Stop event requires JSON output (plain text is invalid).

## Environment Variables

No dedicated environment variables are set during hook execution (unlike Claude Code's `CLAUDE_PROJECT_DIR`). All context is passed via stdin JSON. The `cwd` field serves as the project directory equivalent. `CODEX_HOME` (defaults to `~/.codex`) controls where Codex stores config and state.

## Custom Instructions

| Scope | Path |
|---|---|
| Global | `~/.codex/AGENTS.md` (or `AGENTS.override.md`) |
| Project | `AGENTS.md` (or `AGENTS.override.md`) at each directory level from git root to CWD |

The `project_doc_fallback_filenames` config option in `~/.codex/config.toml` allows alternative filenames. Max combined size: 32 KiB (`project_doc_max_bytes`).

## Skills

| Scope | Path |
|---|---|
| Repository | `.agents/skills/<name>/SKILL.md` (each dir from CWD up to repo root) |
| User | `~/.agents/skills/<name>/SKILL.md` |
| Admin | `/etc/codex/skills/<name>/SKILL.md` |
| System | Bundled with Codex |

Skills use `SKILL.md` with YAML frontmatter (`name`, `description`) and may include `scripts/`, `references/`, `assets/`, and `agents/openai.yaml`.

## MCP Server Configuration

Configured in `~/.codex/config.toml` or `.codex/config.toml` under `[mcp_servers.<name>]`:

```toml
[mcp_servers.my-server]
command = "path/to/executable"
args = ["--arg1"]
env = { API_KEY = "value" }
startup_timeout_sec = 10
tool_timeout_sec = 60
```

Supports stdio (`command`/`args`) and streamable HTTP (`url`/`bearer_token_env_var`). CLI management: `codex mcp add <name> ...`.

## MCP Server Registration

In addition to hooks, symposium registers itself as an MCP server in the
agent's config file. This provides an alternative integration path
alongside the hook-based approach.

### Configuration structure

The MCP server entry is added under `[mcp_servers]` in the TOML config:

```toml
[mcp_servers.symposium]
command = "/path/to/cargo-agents"
args = ["mcp"]
```

- **Project-level**: `.codex/config.toml`
- **User-level**: `~/.codex/config.toml`

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.

## Plugin Delivery

A plugin enabled with `use --global` reaches Codex CLI as Codex's own
`codex plugin marketplace add` and `codex plugin add` would install it, with
symposium writing the same files itself:

```toml
# ~/.codex/config.toml
[marketplaces.symposium]
source_type = "local"
source = "/home/user/.symposium/installed"

[plugins."<plugin>@symposium"]
enabled = true
```

plus a copy of the compiled directory in Codex's plugin cache,
`~/.codex/plugins/cache/symposium/<plugin>/<version>/`. All of it lives under
`$CODEX_HOME` when that variable is set.

- **Marketplace**: the staging root, whose `.claude-plugin/marketplace.json`
  Codex reads. The entry exists only while that manifest does: a registered
  local marketplace without one makes `codex plugin list` fail. A
  `[marketplaces.symposium]` pointing anywhere else is someone else's, so
  symposium installs nothing for Codex and keeps its per-skill copies.
- **Enablement**: one `[plugins."<plugin>@symposium"]` per plugin. An existing
  entry is kept as it is, so `enabled = false` written by `/plugins` stays,
  and `<x>@symposium` entries for plugins no longer delivered are removed.
- **Cache**: sessions load a plugin only from here, never from the marketplace
  source, and Codex's own startup refresh recopies it only when the version
  directory differs from the manifest version, which a same-version edit never
  does. Codex loads the highest-sorting directory under `<plugin>/`, so
  symposium swaps the whole `<plugin>/` directory, staged beside it, and leaves
  exactly one version directory, named after the manifest version (`0.0.0`).
  A copy Codex installed itself, with `codex plugin add` or its startup
  refresh, has no `.symposium` marker in `<plugin>/`; the next sync takes it
  back.

The cache is written before its plugin is enabled, and enablement is removed
before its cache, so a session starting mid-sync never sees an enabled plugin
without its files. `config.toml` is edited in place with everything else
preserved, written only when it changed, atomically, and through a symlink to
its target, as Codex does. A file that does not parse is an error and stays
untouched.

Verified against Codex CLI 0.159.3 by asking the agent (`codex debug
prompt-input`, the model request a session sends, `codex plugin list`, and the
TUI's `/skills` and `/plugins`) after `cargo agents sync`:

- The plugin is listed as installed and enabled, `/plugins` shows it under a
  `symposium` tab, and the model sees its skills as `<plugin>:<skill>` in TUI
  and `codex exec` sessions alike, with no warning.
- An edit at the same version reaches the next session.
- A TUI session leaves symposium's entries, the cache and the staging root
  byte-identical, so the next sync writes nothing.
- Codex runs the `SessionStart` hook when the first prompt is submitted, after
  the session's skills are listed, so a plugin installed by the auto-sync
  appears in the next session.
- Dropping the `use --global` entry, or removing the agent, leaves nothing
  listed and `codex plugin list` working.
- Codex does not read Claude Code's copy in `~/.claude/skills/`.

Known limitations:

- `CODEX_HOME` relocates plugins only: symposium's hooks and MCP server are
  still registered under `~/.codex/`.
- `.agents/skills/` is shared with Antigravity, Copilot, OpenCode and Goose. A
  per-skill copy one of them still needs stays in the project, and Codex then
  lists that skill twice, as `<skill>` and as `<plugin>:<skill>`.
- `codex plugin remove` is undone by the next sync; `cargo agents use --remove`
  removes the plugin, and `/plugins` turns it off.

### Project scope

Codex reads `[marketplaces.*]` and `[plugins.*]` from a project's
`.codex/config.toml` only once the user trusts the project, ignoring an
untrusted one without a word, and even then installs into the user-level cache
keyed by marketplace name, so two repositories using the same name share one
copy. Symposium does not write project-scoped plugins for Codex; they keep
their per-skill copies in `.agents/skills/`.

## Other Extensibility

- `notify` in config.toml (fire-and-forget on agent-turn-complete)
- Execpolicy command-level rules
- Subagents
- Slash commands
