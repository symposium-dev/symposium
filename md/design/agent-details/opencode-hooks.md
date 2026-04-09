# OpenCode Hooks Reference

OpenCode's extensibility centers on TypeScript/JavaScript plugins, not shell commands. Plugins are async functions that receive a context object and return a hooks object. A secondary experimental system supports shell-command hooks in `opencode.json`.

## Plugin Locations and Load Order

Hooks run sequentially in this order:

1. Global config plugins (`~/.config/opencode/opencode.json` → `"plugin": [...]`)
2. Project config plugins (`opencode.json`)
3. Global plugin directory (`~/.config/opencode/plugins/`)
4. Project plugin directory (`.opencode/plugins/`)

npm packages are auto-installed via Bun and cached in `~/.cache/opencode/node_modules/`.

## Plugin Context Object

All plugins receive: `{ project, client, $, directory, worktree }`.

## Core Plugin Hooks

| Hook | Trigger | Control Flow |
|---|---|---|
| `event` | Every system event (~30 types including `session.idle`, `session.created`, `tool.execute.before`, `file.edited`, `permission.asked`) | Observe only |
| `tool.execute.before` | Before any built-in tool runs | **Throw Error → block**. Mutate `output.args` → modify tool arguments. Return normally → allow. |
| `tool.execute.after` | After a built-in tool completes | Mutate `output.title`, `output.output`, `output.metadata` |
| `shell.env` | Before any shell execution | Mutate `output.env` to inject environment variables |
| `stop` | Agent attempts to stop | Call `client.session.prompt()` to prevent stopping and send more work |
| `config` | During configuration loading | Mutate config object directly |
| `tool` | Plugin load time (declarative) | Registers custom tools via `tool()` definitions; overrides built-ins with same name |
| `auth` | Auth initialization | Object with `provider`, `loader`, `methods` |
| `chat.message` | Chat message processing | Mutate `message` and `parts` via output object |
| `chat.params` | Before LLM API call | Mutate `temperature`, `topP`, `options` via output object |
| `permission.ask` | Permission requested | Set `output.status` to `'allow'` or `'deny'` — **reportedly never called (bug #7006)** |

## Experimental Hooks (prefix `experimental.`)

| Hook | Description |
|---|---|
| `chat.system.transform` | Push strings to `output.system` array to augment system prompt |
| `chat.messages.transform` | Mutate `output.messages` array |
| `session.compacting` | Push to `output.context` or replace `output.prompt` during compaction |

## tool.execute.before Schema

### Input

```json
{
  "tool": "string",
  "sessionID": "string",
  "callID": "string"
}
```

### Output (mutable)

```json
{
  "args": { "key": "value" }
}
```

Mutate `output.args` to change tool arguments before execution.

## chat.params Schema

### Input

```json
{
  "model": "string",
  "provider": "string",
  "message": "object"
}
```

### Output (mutable)

```json
{
  "temperature": 0.7,
  "topP": 0.9,
  "options": {}
}
```

## Limitations

- **MCP tool calls do NOT trigger `tool.execute.before` or `tool.execute.after`** (issue #2319).
- Plugin-level syntax errors prevent loading entirely.
- `tool.execute.before` errors block the tool.
- No explicit timeout documentation for plugin hooks.
- No hook ordering guarantees beyond load order.

## Experimental Config-Based Shell Hooks (opencode.json)

A simpler shell-command system under `"experimental.hook"`:

```json
{
  "experimental": {
    "hook": {
      "file_edited": {
        "*.ts": [{ "command": ["prettier", "--write"], "environment": {"NODE_ENV": "development"} }]
      },
      "session_completed": [{ "command": ["notify-send", "Done!"], "environment": {} }]
    }
  }
}
```

Only two events: **`file_edited`** (glob-matched) and **`session_completed`**. No `session_start` (requested in issue #12110).

## Environment Variables

**Core OpenCode** sets these on child processes:
- `OPENCODE_SESSION_ID` — current session identifier
- `OPENCODE_SESSION_TITLE` — human-readable session name

The `shell.env` plugin hook allows injecting custom environment variables into all shell execution.

Configuration-related env vars (not hook-specific): `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, `OPENCODE_MODEL`.

## Custom Instructions

| Scope | Path |
|---|---|
| Project | `AGENTS.md` at project root |
| Global | `~/.config/opencode/AGENTS.md` |
| Legacy compat | `CLAUDE.md` (project), `~/.claude/CLAUDE.md` (global) |
| Config-based | `"instructions"` array in `opencode.json` (file paths and globs) |

Priority: local `AGENTS.md` > local `CLAUDE.md` > global `~/.config/opencode/AGENTS.md` > global `~/.claude/CLAUDE.md`.

## Skills

| Scope | Path |
|---|---|
| Project | `.opencode/skills/`, `.claude/skills/`, `.agents/skills/` |
| Global | `~/.config/opencode/skills/`, `~/.claude/skills/`, `~/.agents/skills/` |

OpenCode walks up from CWD to the git worktree root, loading matching skill definitions. Skills use `SKILL.md` with YAML frontmatter (`name`, `description`) and are loaded on-demand via the native `skill` tool.

## Additional Events (Plugin System)

The full event list includes: `session.created`, `session.idle`, `session.compacted`, `message.updated`, `file.edited`, `file.watcher.updated`, `permission.asked`, `permission.replied`, `tool.execute.before`, `tool.execute.after`, `shell.env`, `tui.prompt.append`, `tui.command.execute`, and others (~30 total). The `message.updated` event (filtered by `role === "user"`) is the closest equivalent to a user-prompt-submit hook. The `session.created` event is the session-start equivalent.
