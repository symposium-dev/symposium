# Antigravity CLI Hooks Reference

<!-- Proposed content for md/design/agent-details/antigravity-cli.md -->

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

**Headless print mode is the exception.** `agy -p` adopts no workspace unless
given `--add-dir <absolute path>` — it cannot read project files and loads no
project `.agents/` configuration. A relative `--add-dir .` is ignored. Automation
driving `agy -p` against a project must pass an absolute path.

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
      { "type": "command", "command": "cargo-agents hook antigravity pre-invocation" }
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
| `PostInvocation` | after tool calls finish | n/a |
| `Stop` | when the execution loop terminates | n/a |
| `SessionStart` | once per session | n/a |

`SessionStart` is **absent from the official documentation** but present in the
binary's hook proto, and works: it loads from `hooks.json`, fires once per
session, and carries a populated `workspacePaths`. `PreInvocation` by contrast
fires before every model call, and its `invocationNum` restarts at 0 each turn,
so it marks the start of a turn rather than of a session.

Unknown event keys are accepted silently and never fire, so a misspelled event
name fails without any error.

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

`workspacePaths` is populated once the workspace is adopted, and **empty** under
`agy -p` without `--add-dir`. A hook's working directory is always the directory
holding its own `hooks.json`, never the user's project.

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
