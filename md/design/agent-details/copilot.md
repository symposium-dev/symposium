# GitHub Copilot Hooks Reference

> **Disclaimer:** This document reflects our current understanding of GitHub Copilot's hook system.
> It is a working reference for symposium development, not a substitute for the official docs.
> Details may be outdated or incomplete — always consult the primary sources.
>
> **Primary sources:**
> [About hooks](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-hooks)
> · [Using hooks](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/use-hooks)
> · [Hooks configuration](https://docs.github.com/en/copilot/reference/hooks-configuration)

GitHub Copilot hooks are available for the Cloud Agent (coding agent), Copilot CLI (GA February 2026), and VS Code (8-event preview). The system is command-only and repository-native.

## Hook Types

Only `type: "command"` is supported.

## Events

### Cloud Agent and CLI (6 events, lowerCamelCase)

| Event | Trigger | Can block? |
|---|---|---|
| `sessionStart` | New or resumed session | No |
| `sessionEnd` | Session completes or terminates | No |
| `userPromptSubmitted` | User submits a prompt | No |
| `preToolUse` | Before tool call | **Yes** |
| `postToolUse` | After tool completes (success or failure) | No |
| `errorOccurred` | Error during agent execution | No |

### VS Code (8 events, PascalCase, preview)

`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PreCompact`, `SubagentStart`, `SubagentStop`, `Stop`.

Only **`preToolUse`/`PreToolUse`** can make access-control decisions. All other events are observational, though they can still add context (see [what the CLI actually reads](#what-the-cli-actually-reads)).

## Configuration

### Cloud Agent and CLI

Hooks defined in **`.github/hooks/*.json`**. For the Cloud Agent, files must be on the repository's **default branch**.

The CLI combines hooks from several places: repository `.github/hooks/*.json`, user-level `~/.copilot/hooks/*.json` or a `hooks` key in `~/.copilot/settings.json` (where symposium registers globally; `config.json` is managed by the CLI), and also `.claude/settings.json`. It loads a repository's hooks only once the folder is trusted: it asks interactively, or `COPILOT_ALLOW_ALL=true` trusts the working directory. Because it reads Claude's settings too, a project with both agents configured runs symposium's hook twice per Copilot event.

`COPILOT_HOME` moves the user level as a whole: when it is set, the CLI reads `settings.json` and `mcp-config.json` from `$COPILOT_HOME` instead of `~/.copilot/`, so symposium writes its global hooks and MCP servers there. Verified against the Copilot CLI 1.0.89 with the variable set: symposium's hooks and MCP server left in `~/.copilot/` were ignored (`copilot mcp list` reported no server and no hook ran), while with them under `$COPILOT_HOME` each event ran `cargo-agents hook copilot`, and `copilot mcp list` and the `/mcp` dashboard showed the server, which a session started and offered as a tool. The CLI still keeps its cache of each MCP server's tools in `~/Library/Caches/copilot/mcp-tools/` on macOS, which the variable does not move.

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "./scripts/security-check.sh",
        "powershell": "./scripts/security-check.ps1",
        "cwd": "scripts",
        "env": { "LOG_LEVEL": "INFO" },
        "timeoutSec": 15,
        "comment": "Documentation string, ignored at runtime"
      }
    ]
  }
}
```

| Field | Type | Description |
|---|---|---|
| `type` | string | Must be `"command"` |
| `bash` | string | Command for Linux/macOS |
| `powershell` | string | Command for Windows |
| `cwd` | string | Working directory relative to repo root |
| `env` | object | Environment variables |
| `timeoutSec` | number | Default **30 seconds** |
| `comment` | string | Documentation, ignored at runtime |

There is **no `matcher` field** — hooks fire on all invocations of their event type. Tool-level filtering must be done inside the script by inspecting `toolName` from stdin.

### VS Code

Also reads hooks from `.claude/settings.json`, `.claude/settings.local.json`, and `~/.claude/settings.json` (Claude Code format compatibility). Converts lowerCamelCase to PascalCase and maps `bash`→`osx`/`linux`, `powershell`→`windows`.

## Input Schema (stdin)

### preToolUse

```json
{
  "timestamp": 1704614600000,
  "cwd": "/path/to/project",
  "toolName": "bash",
  "toolArgs": "{\"command\":\"rm -rf dist\",\"description\":\"Clean build\"}"
}
```

**Note**: `toolArgs` is a **JSON string**, not an object. Scripts must parse it (e.g., with `jq`).

### sessionStart

- `source`: `"new"` | `"resume"`
- `initialPrompt`: string

### sessionEnd

- `reason`: string

## Output Schema (stdout)

### preToolUse output (Cloud Agent / CLI)

```json
{
  "permissionDecision": "deny",
  "permissionDecisionReason": "Destructive operations blocked"
}
```

| Value | Meaning |
|---|---|
| `"allow"` | Permit the tool call |
| `"deny"` | Block the tool call |
| `"ask"` | Prompt user for confirmation |

Exit code 0 = allow (if no JSON output), non-zero = deny.

### What the CLI actually reads

Observed on Copilot CLI 1.0.80 (2026-09-29), with a hook per event returning a different marker and the model asked which markers it saw:

- A top-level `additionalContext` reached the model from `sessionStart`, `userPromptSubmitted`, `preToolUse` and `postToolUse`. GitHub's hooks reference says otherwise for two of them: that `userPromptSubmitted` output from config-file hooks is dropped, and that `preToolUse` takes no context.
- Output that is not pure JSON (for example, a progress line ahead of the object) is logged and dropped, on every event. Nothing reaches the model, not even at session start.
- `preToolUse` honors `permissionDecision` and `permissionDecisionReason`, and `modifiedArgs` rewrites the call.
- The shell tool is named `bash` (lowercase) in the hook input.

### VS Code output (preview, extended fields)

| Field | Type | Description |
|---|---|---|
| `continue` | boolean | `false` stops agent |
| `stopReason` | string | Message when continue is false |
| `systemMessage` | string | Warning shown to user |
| `hookSpecificOutput.permissionDecision` | string | `allow`, `deny`, `ask` |
| `hookSpecificOutput.updatedInput` | object | Replace tool arguments |
| `hookSpecificOutput.additionalContext` | string | Extra context for agent |

## Execution Behavior

- Hooks run **synchronously and sequentially** (array order).
- If the first hook returns `deny`, subsequent hooks are **skipped**.
- Recommended execution time: **under 5 seconds**.
- Default timeout: **30 seconds**. On timeout, hook is terminated and agent continues.
- Scripts read JSON from stdin (`INPUT=$(cat)`) and write to stdout; debug output goes to stderr.

## Environment Variables

No built-in variables beyond those specified in the hook's `env` field. The `cwd` field controls the working directory.

## Additional Extension Surfaces (not hooks, but related)

### Custom instructions (soft/probabilistic)

| File | Scope |
|---|---|
| `.github/copilot-instructions.md` | Repository-wide instructions |
| `.github/instructions/**/*.instructions.md` | Path-specific instructions (with `applyTo` globs) |
| `AGENTS.md` | Agent-mode instructions |
| `~/.copilot/copilot-instructions.md` | User-level (personal) |
| Organization-level instructions | Admin-configured |

Priority: Personal (user) > Repository (workspace) > Organization.

### MCP server configuration

| Scope | Config path | Root key |
|---|---|---|
| VS Code (workspace) | `.vscode/mcp.json` | `servers` |
| VS Code (user) | Via "MCP: Open User Configuration" command | `servers` |
| CLI | `~/.copilot/mcp-config.json` | `mcpServers` |

Note: VS Code uses `"servers"` as root key while the CLI uses `"mcpServers"`. MCP tools only work in Copilot's Agent mode. Supported transports: `local`/`stdio`, `http`/`sse`.

### MCP Server Registration

Symposium registers MCP servers in the Copilot config as top-level keys
(matching the CLI's `mcpServers` format, not VS Code's `servers` format):

```json
{
  "symposium": {
    "command": "/path/to/cargo-agents",
    "args": ["mcp"]
  }
}
```

- **Project-level**: `.vscode/mcp.json`
- **User-level**: `~/.copilot/mcp-config.json` (`$COPILOT_HOME/mcp-config.json` when set)

Registration is idempotent — if the entry already exists with the
correct values, no changes are made. If the entry exists but has stale
values (e.g. the binary moved), it is updated in place.

### Copilot SDK (programmatic hooks)

The `@github/copilot-sdk` (Node.js, Python, Go, .NET, Java) provides callback-style hooks for applications embedding the Copilot runtime:

- `onPreToolUse` — can return `modifiedArgs`
- `onPostToolUse` — can return `modifiedResult`
- `onSessionStart`, `onSessionEnd`, etc.

### Agent firewall (Cloud Agent)

Network-layer control with deny-by-default domain allowlist, configured at org/repo level. Not a hook — controls outbound network access.

## Plugin Delivery

A plugin enabled with `use --global` reaches the Copilot CLI through its user settings, `~/.copilot/settings.json` (`$COPILOT_HOME/settings.json` when that variable is set). While Codex or Copilot has plugins, symposium writes `.claude-plugin/marketplace.json` into the staging root `~/.symposium/installed/`, registers that root as a directory marketplace and enables each plugin from it:

```json
{
  "extraKnownMarketplaces": {
    "symposium": {
      "source": { "source": "directory", "path": "/home/alex/.symposium/installed" }
    }
  },
  "enabledPlugins": {
    "pdf-tools@symposium": true
  }
}
```

These are the entries `copilot plugin marketplace add` and `copilot plugin install pdf-tools@symposium` write for a directory marketplace, and nothing else: no `installedPlugins` record in `config.json` and no copy under `~/.copilot/installed-plugins/`. Symposium owns only the `symposium` marketplace entry and the `<plugin>@symposium` keys, and keeps a `false`, which is what `copilot plugin disable` and `copilot plugin uninstall` write. A file it cannot read, or a `symposium` marketplace pointing elsewhere, is left untouched with a warning, and the skills arrive per project instead.

Verified against the Copilot CLI 1.0.89 by asking the agent (`copilot plugin list --json`, `copilot skill list --json`, the `/skills` dashboard, and the skills a `-p` session offers its model):

- The plugin loads in place in the first session after `cargo agents sync`, interactive and `-p` alike, and its skills keep their plain names (`/<skill>`). An edit at the same version reaches the next session.
- Removing the entries and the directory leaves nothing behind. An entry left for a missing root makes every session warn that the marketplace was skipped.
- A plugin registered during the `sessionStart` hook is offered after `/restart` or in the next session; `/skills reload` does not pick it up.
- A skill of the same name in `~/.copilot/skills/`, `~/.agents/skills/` or the project's `.agents/skills/` silently replaces the plugin's, including a per-skill copy symposium writes there for another agent.
- Claude Code's user-level skills-directory plugins (`~/.claude/skills/<plugin>/`) do not reach Copilot; its project `.claude/settings.json` registrations and `.claude/skills/<plugin>/` directories do.
- Copilot CLI 1.0.79 and 1.0.80 load such a plugin only after an interactive session installs a copy of its own, which goes stale and outlives the entries. Copilot updates itself by default.
- A `.github/copilot/settings.local.json` at the git root holding the same entries, under a marketplace name other than the user-level one, scopes a plugin to that repository. Symposium does not write it.
