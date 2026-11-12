# Hooks

Symposium's hook system is guided by the project [tenets](./tenets.md): symposium is always the intermediary, it never dirties the user's repo, and portability is the default.

## Hook formats

A plugin hook declares which wire format its handler expects:

- `format = "symposium"` (default) — the handler receives symposium canonical JSON. This is portable across all agents.
- `format = "antigravity"` / `"claude"` / `"copilot"` / `"codex"` / `"kiro"` — the handler receives that agent's native wire format.

## Dispatch rule

When symposium's global handler receives an event from agent A, it loads all plugins and finds each plugin's candidate hooks: those whose event and `matcher` match, whose `agent`, if set, is A, and whose `predicates` hold. For each plugin, it picks one format and delivers to every candidate of that format, in manifest order:

1. If the plugin declares hooks with `format` matching **agent A**, each receives the input unmodified (the handler already expects this agent's native format).
2. Otherwise, if the plugin declares **symposium-format** hooks, each receives the input converted to symposium canonical.
3. Otherwise, nothing fires for this plugin.

A plugin can hold several handlers for one event, such as a blocking check and a logger imported from a Claude Code plugin, so they all run. A native match still replaces the plugin's symposium-format hooks for that event: they are its fallback for other agents, not extra handlers.

Symposium never converts between agent-specific formats. A `format = "claude"` hook will only fire on Claude — it won't be translated for Copilot or Antigravity. If you want cross-agent coverage, provide a symposium-format hook as a fallback.

### Example

A plugin with hooks for `claude`, `antigravity`, and `symposium`:
- On Claude: the `format = "claude"` hook receives Claude's native JSON.
- On Antigravity: the `format = "antigravity"` hook receives Antigravity's native JSON.
- On Copilot: no native handler → the `format = "symposium"` hook receives symposium canonical JSON.

A plugin with only `format = "symposium"`:
- Works on all agents. Symposium converts the agent's wire format to canonical before delivering.

A plugin with only `format = "claude"`:
- On Claude: receives Claude's native JSON directly.
- On other agents: nothing fires (no symposium fallback declared).

## Output handling

Symposium converts the hook's output back to the current agent's wire format before returning it to the agent:

- Native format matching the host agent: pass through directly. A native Claude or Codex `SessionStart` or `UserPromptSubmit` hook may print plain text instead of JSON; Symposium adds it to the context, as those agents do.
- Symposium format → convert to host agent's wire format.

Stdout from a symposium-format hook that is not a canonical output, such as `null` or an object without the event tag, is ignored with a warning, like invalid JSON. Only translated output reaches the agent, so one plugin cannot replace or clear what the others returned.

## Time limits and cancellation

A hook runs inside the single `cargo agents hook` call the agent is waiting for, so every plugin hook of one event shares an 8-second budget, and each one gets at most 5 seconds of it (see [the hook flow](./hook-flow.md)). The budget covers preparing the hook's command too: acquiring its source and running its `install_commands`. A `cargo binstall` or `cargo install` that acquisition started, and the `git ls-remote` that checks a git source, is killed with everything it started when acquisition stops at the deadline, so no installer keeps running, or keeps symposium from exiting, after the hook is skipped.

Stopping a hook has to stop everything it started, or a timed-out hook keeps changing the workspace after the agent has moved on, and a child holding symposium's stderr keeps the agent waiting. So on Unix each hook, and each `install_commands` entry run during dispatch, gets its own process group, and a timeout kills the whole group; on Windows, `taskkill /T` kills the process tree. `symposium_install::process_tree` holds both. Outside hook dispatch (subcommands, `sync`, the `SessionStart` prewarm), `install_commands` and the installers acquisition starts stay in symposium's group (`InstallContext::in_foreground`), so Ctrl-C and terminal prompts still reach them. The `git ls-remote` probe never prompts (`GIT_TERMINAL_PROMPT=0`), since a prompt from a group of its own would stop it instead.

A hook that exits on its own is done: background jobs it leaves running are not killed or waited for.

On Antigravity, a native `PreToolUse` object with no `decision` (or an empty one) is read as the denial Antigravity takes it to be, and injected steps other than context (`userMessage`, `toolCall`) accumulate across hooks.

A hook that exits 2 blocks, and the block reaches each agent in the form it acts on. Antigravity reads nothing from an exit code, so it receives a denial on `PreToolUse` and, on `Stop`, `decision: "continue"` with the hook's stderr as the reason; Claude Code and Codex continue a turn on exit 2 itself.

## Alternatives considered

### Registering agent-specific hooks directly

An earlier design had symposium write plugin hook commands directly into agent configuration files (e.g., `.claude/settings.json`, `.github/hooks/*.json`) at sync time. The agent would invoke them natively, and symposium's global handler would skip delivery for those plugins.

We rejected this because it violates the [Unobtrusive](./tenets.md#unobtrusive) tenet:

- Agent config files are often git-tracked. Writing plugin hooks into them creates unexpected diffs that pollute pull requests and cause merge conflicts.
- Users would need to `.gitignore` symposium-managed entries, or accept noise in their version history.
- It couples symposium's state to files the user considers "theirs," making it harder to adopt or remove symposium cleanly.

The current design avoids these problems by keeping symposium as the sole registered hook handler. Plugin hooks are dispatched internally — the agent's config only ever contains one symposium entry, registered at init time.

### Cross-agent format conversion

We also considered converting between agent-specific formats (e.g., delivering a `format = "claude"` hook on Copilot by translating Copilot's input into Claude's format). We rejected this because:

- The conversion is lossy — agents have different fields, semantics, and capabilities.
- It creates surprising behavior: a hook author declares `format = "claude"` expecting Claude's schema, but receives a synthetic approximation on other agents.
- It's simpler and more predictable to require a symposium-format fallback for cross-agent coverage.
