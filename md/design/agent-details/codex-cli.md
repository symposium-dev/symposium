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

`CODEX_HOME` moves the user level as a whole: when it is set, Codex reads
`$CODEX_HOME/hooks.json` and `$CODEX_HOME/config.toml`, and ignores
`~/.codex/hooks.json` and `~/.codex/config.toml`. Symposium writes its global
hooks and MCP servers there too. Verified against Codex CLI 0.159.3 with the
variable set: the TUI's hook review listed symposium's four hooks with
`$CODEX_HOME/hooks.json` as their source, and `codex mcp list` showed its
server.

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
- **User-level**: `~/.codex/config.toml` (`$CODEX_HOME/config.toml` when set)

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.

## Plugin Delivery

A plugin enabled with `use --global` reaches Codex CLI as an installed plugin,
the way `codex plugin marketplace add` and `codex plugin add` install one.
Symposium writes, under `~/.codex/` (`$CODEX_HOME` when set):

```toml
# config.toml
[marketplaces.symposium]
source_type = "local"
source = "/home/user/.symposium/installed"

[plugins."<plugin>@symposium"]
enabled = true
```

and a copy of the compiled directory in Codex's plugin cache,
`plugins/cache/symposium/<plugin>/0.0.0/`, named after the manifest version.

- The marketplace is the staging root, whose `.claude-plugin/marketplace.json`
  symposium writes while Codex or the Copilot CLI has plugins. A `symposium`
  marketplace pointing anywhere else is someone else's: symposium warns and
  Codex gets per-skill copies.
- An existing plugin entry is left as it is, so `enabled = false` (Space in
  `/plugins`) keeps the plugin off, and once the plugin is no longer delivered
  only an entry still set to `true` is removed.
- The rest of `config.toml` is kept; a file that does not parse is reported,
  left untouched, and Codex gets per-skill copies.

Verified against Codex CLI 0.159.3 by asking the agent (`codex debug
prompt-input`, the model request a session sends, `codex plugin list`, and
the TUI's `/skills` and `/plugins`) after `cargo agents sync`, with Codex as
the only agent and with the Copilot CLI configured too:

- `codex plugin list` shows `<plugin>@symposium` installed and enabled,
  `/plugins` lists it under a `symposium` tab, and the model sees its skills
  as `<plugin>:<skill>` in TUI and `codex exec` sessions alike.
- Sessions load a plugin only from the cache. Codex's startup refresh copies
  it again only when the version directory differs from the manifest version,
  and `codex exec` never fills it, so symposium writes the copy itself; an
  edit reaches the next session.
- A registered local marketplace whose root has no manifest makes
  `codex plugin list` fail, so the index exists as long as a reader has
  plugins.
- `codex plugin remove` deletes the plugin entry and the cache copy; the next
  sync installs it again. Removing the `use --global` entry leaves
  `config.toml` as it was.
- A plugin installed by the `SessionStart` hook appears in the next session:
  Codex runs that hook when the first prompt is submitted, after the
  session's skills are listed.

## Other Extensibility

- `notify` in config.toml (fire-and-forget on agent-turn-complete)
- Execpolicy command-level rules
- Subagents
- Slash commands
