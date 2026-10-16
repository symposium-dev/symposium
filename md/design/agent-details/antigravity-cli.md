# Antigravity CLI Hooks Reference

> **Disclaimer:** This document reflects our current understanding of Antigravity CLI's
> hook system. It is a working reference for symposium development, not a substitute for
> the official docs. Details may be outdated or incomplete — always consult the primary
> sources.
>
> **Primary sources:**
> [Hooks](https://antigravity.google/docs/hooks/)
> · [Plugins & Skills](https://antigravity.google/docs/cli/plugins/)
> · [GitHub repo](https://github.com/google-antigravity/antigravity-cli)
> · the bundled `agy-customizations` skill under
> `~/.gemini/antigravity-cli/builtin/skills/`, which is more precise than the website

Google's Antigravity CLI (`agy`) exposes shell-command hooks through a dedicated
`hooks.json`. Antigravity ships three surfaces — the CLI, the IDE, and the web
app — which share the same configuration roots.

## Configuration

| File | Scope |
|---|---|
| `~/.gemini/config/hooks.json` | User-global |
| `<repo>/.agents/hooks.json` | Project-scoped |

Both are additive; all matching hooks run, each with its working directory set to
the directory containing its own `hooks.json`.

Project scope works in ordinary interactive use: `agy` walks up from the working
directory to find `.agents/`, and loads project hooks on a second pass once it has
adopted the workspace.

Headless print mode (`agy -p`) adopts the repository it starts in the same way,
without `--add-dir`: project hooks fire with `workspacePaths` populated and
project plugins load. It loads project plugins in a folder the user never trusted,
too, since the trust dialog only guards the interactive TUI. Verified on 1.2.14;
1.2.7 adopted no workspace without `--add-dir <absolute path>`.

### Configuration structure

Each top-level key is a **hook name** mapping to its events. `PreToolUse` and
`PostToolUse` wrap handlers in a `matcher` group; `PreInvocation`,
`PostInvocation` and `Stop` take flat handler lists.

```json
{
  "symposium": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": "cargo-agents hook antigravity pre-tool-use", "timeout": 30 }
        ]
      }
    ],
    "PreInvocation": [
      { "type": "command", "command": "cargo-agents hook antigravity user-prompt-submit", "timeout": 30 }
    ]
  }
}
```

`enabled: false` on a named hook disables all its handlers. `timeout` is in
**seconds** and defaults to 30. Only `type: "command"` is supported; commands run
via `sh -c` (`cmd /c` on Windows).

## Events

| Event | When it fires | Matcher |
|---|---|---|
| `PreToolUse` | before a tool step executes | tool name |
| `PostToolUse` | after a tool step completes | tool name |
| `PreInvocation` | before the model is called | n/a |
| `PostInvocation` | after each model invocation completes | n/a |
| `Stop` | when the execution loop terminates | n/a |
| `SessionStart` | with the first turn of a new conversation | n/a |

`SessionStart` is **absent from the official documentation** but present in the
binary's hook proto (the `HookArgs` oneof has exactly six variants: `PreTool`,
`PostTool`, `PreInvocation`, `PostInvocation`, `SessionStart`, `Stop`), and it
works: registered in `hooks.json` it loads and carries a populated
`workspacePaths`. It fires with the **first turn of a new conversation**, in the
TUI at the first prompt rather than at launch. Resuming a conversation (`-c`,
`--conversation`, or the TUI) starts a new process that never fires it, so a
resumed conversation skips symposium's `SessionStart` refresh until a new
conversation starts. `PreInvocation` by contrast fires before every model call,
and its `invocationNum` restarts at 0 each turn, so it marks the start of a turn
rather than of a session. That is why symposium maps it to `user-prompt-submit`
and dispatches only `invocationNum == 0` (`AgentHookInput::should_dispatch`).
Verified on agy 1.2.7.

Unknown event keys are accepted silently and never fire, so a misspelled event
name fails with no error at all.

Matchers are regexes over tool names, which are the lowercased step type without
its `CORTEX_STEP_TYPE_` prefix — `run_command`, `view_file`, `browser_.*`. A
matcher written for another agent's tool names (`Bash`, `Edit`) will not match.

## Input Schema (stdin)

All keys are camelCase (protojson).

### Base fields (all events)

```json
{
  "conversationId": "5e4f131c-…",
  "workspacePaths": ["/path/to/workspace"],
  "transcriptPath": "…/transcript_full.jsonl",
  "artifactDirectoryPath": "…",
  "modelName": "gemini-3.7-flash-high"
}
```

`workspacePaths` is populated once the workspace is adopted, in print mode too.
A hook's working directory is always the directory holding its own `hooks.json`,
never the user's project.

### PreToolUse / PostToolUse additions

```json
{
  "toolCall": { "name": "run_command", "args": { "CommandLine": "npm test" } },
  "stepIdx": 2,
  "error": ""
}
```

`error` is present on `PostToolUse` only. Both events carry `toolCall`.

### PreInvocation / PostInvocation additions

```json
{ "invocationNum": 0, "initialNumSteps": 1 }
```

### Stop additions

```json
{ "executionNum": 0, "terminationReason": "NO_TOOL_CALL", "error": "", "fullyIdle": true }
```

## Output Schema (stdout)

### PreToolUse

```json
{ "decision": "allow", "reason": "optional", "permissionOverrides": [] }
```

`decision` is required: `allow`, `deny`, `ask`, `force_ask`, or
`deny_unless_prior_grant`. An `overwrite` object shallow-merges into the tool
call's arguments before it runs.

**Writing `{}` denies the call.** So do `{"decision": ""}` and objects carrying
only other fields. Writing nothing at all allows. Any hook that does not intend to
block must emit an explicit `{"decision": "allow"}`.

### PostToolUse

Expects `{}`.

### PreInvocation — inject context

```json
{ "injectSteps": [{ "ephemeralMessage": "..." }] }
```

Each step accepts one of `toolCall`, `userMessage`, or `ephemeralMessage`. This is
the equivalent of Claude Code's `additionalContext`.

### PostInvocation

`injectSteps` as above, plus `terminationBehavior`: `force_continue`, `terminate`,
or omitted.

### Stop

```json
{ "decision": "continue", "reason": "required when continuing" }
```

Any value other than `continue` lets the agent stop.

## Exit Codes

**Ignored.** Exit 1 and exit 2 behave exactly as exit 0; only stdout decides the
outcome. This differs from every other agent symposium supports, where exit 2
blocks.

## Skills

| Scope | Path |
|---|---|
| Project | `<repo>/.agents/skills/<name>/SKILL.md` |
| Global | `~/.gemini/config/skills/<name>/SKILL.md` |

A skill is a directory containing `SKILL.md` with `name` and `description`
frontmatter. Additional files and subdirectories (`scripts/`, `examples/`,
`resources/`, `references/`) are supported, so symposium's `.symposium` marker and
`.gitignore` are preserved. The CLI also reads
`~/.gemini/antigravity-cli/skills/` and `~/.gemini/skills/`, but
`~/.gemini/config/skills/` is the location all three surfaces recognize.

### Registering skills from another location

`~/.gemini/config/skills.json` registers skill directories stored outside the
default locations, and is read on every run:

```json
{ "entries": [{ "path": "/abs/path/to/repo/.agents/skills", "exclude": ["experimental-.*"] }] }
```

Paths must be **absolute**. The schema also documents workspace-relative paths,
but an entry of `.agents/skills` resolves to nothing in practice.

The list is global with no notion of the active repository, so every indexed
directory loads in every session; `include_only` and `exclude` filter by skill
directory name, not by workspace. Symposium does not use this file — project
skills are found by ordinary discovery — but it is the mechanism for skills kept
outside the standard locations.

Symlinks are followed, both a symlinked skill directory inside a skills folder
and a symlink of the folder itself. A separate report of symlinked skills being
ignored concerns the IDE and `~/.gemini/antigravity/skills/`.

`plugins.json` follows the same schema for plugin directories.

## Plugin Delivery

A plugin enabled with `use --global` reaches Antigravity as a copy of the
compiled plugin directory in `~/.gemini/config/plugins/<plugin>/`, the global
folder every Antigravity surface discovers. `agy plugin install <dir>` copies a
directory to the same place, unchanged. Antigravity loads it in every project as
the plugin `<plugin>` and offers its skills as `/<plugin>:<skill>`. The directory
name, not the manifest `name`, is the plugin's identity. The `$schema` and
`version` fields, `.claude-plugin/` and the `.symposium` marker are ignored.

Symposium writes nothing else. Antigravity keeps its own state about the plugins
it finds, which symposium leaves alone: `plugins.<dir>.enabled` in
`~/.gemini/config/config.json` (a user's `false` there hides the plugin;
Antigravity seeds these entries once, for the plugins present the first time it
runs) and `~/.gemini/antigravity-cli/plugin_data/<dir>/`, which it removes itself
once the plugin is gone. `agy plugin list` prints only
`~/.gemini/config/import_manifest.json`, Antigravity's record of its own
installs, so it never shows a plugin symposium installed; `/skills` does.

Verified against agy 1.2.14 through `cargo agents sync`, by asking the agent
(`/skills` in the TUI, and the model request of `agy -p`):

- The plugin is listed in the first session after the sync, as
  `<plugin>:<skill>` from `~/.gemini/config/plugins/<plugin>`, with no bare
  duplicate.
- An edit at the same version reaches the next session: the copy is read in
  place.
- Removing the directory removes the plugin, with no error.
- A plugin written during the `SessionStart` hook (symposium's auto-sync) is not
  offered in that session, also a while later, until `/skills reload` or a new
  session.
- A project plugin of the same name wins over the user copy, silently.
- A per-skill copy in the project's `.agents/skills/` next to the plugin is
  listed twice, as `<skill>` and as `<plugin>:<skill>`.

`~/.gemini/config/plugins.json` can register a plugin directory by path instead,
but a plugin loaded that way lists its skills without the `<plugin>:` prefix and
ranks below the built-in customizations, so symposium copies.

### Project scope

A `<repo>/.agents/plugins/<plugin>/` folder loads as a project plugin, found by
walking up to the git root, in a folder the user has trusted (headless `agy -p`
loads it in an untrusted one too). Two repositories with the same plugin name
each see their own. Goose reads the same folder. Symposium does not write
project-scoped plugins for Antigravity yet.

## MCP server configuration

| Scope | Path |
|---|---|
| Project | `<repo>/.agents/mcp_config.json` |
| Global | `~/.gemini/config/mcp_config.json` |

Standard `mcpServers` object; stdio entries use `command`/`args`/`env`, remote
entries use `serverUrl`. Unlike Gemini CLI, MCP configuration does **not** share a
file with hooks. `agy mcp add` has no scope flag and writes the global file.

## Custom instructions

`GEMINI.md` and `AGENTS.md` at the workspace root, plus `.agents/rules/*.md`,
loaded by walking up to the repository root.
