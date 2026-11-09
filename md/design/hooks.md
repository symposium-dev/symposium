# Hooks

Symposium's plugin hook system is guided by the project [tenets](./tenets.md): symposium dispatches plugin hooks through its registered handler, it never dirties the user's repo, and portability is the default. The SDK also supports standalone hook binaries registered directly with an agent.

## Standalone portable hooks

`symposium_sdk::hook::hook_main(handler)` runs a `HookHandler` directly under an agent. It reads arguments and stdin, converts the native input to the SDK's canonical `Input`, invokes the handler once, and converts its `Output` back to the selected protocol. Agent schemas and conversions live in `symposium_sdk::hook_schema`; Symposium re-exports them from its own `hook_schema` module.

The command-line contract is:

- `--agent=claude`, `codex`, `kiro`, `copilot`, or `antigravity` selects that agent's protocol. Invalid input is an error, without fallback to another agent.
- `--agent=symposium` selects canonical Symposium input and output. The existing `hook::run(handler)` remains a canonical-only entry point.
- Omitting `--agent`, or passing `--agent=guess`, tries Claude Code, then Codex, then Kiro. The first successful input conversion chooses the output protocol too. Overlapping inputs intentionally resolve to Claude Code; this selects a compatible interpretation rather than identifying the calling agent.
- `--event=pre-tool-use` (or an agent's native event name) supplies the event when the input omits it. A conflicting event in the input is an error. Native event names are validated: Claude and Codex use PascalCase, while Kiro uses camelCase.

Fallback only covers input decoding. If no adapter accepts the input, the process reports an error on stderr and exits unsuccessfully. Once the handler runs, neither a handler error nor an output conversion error retries another adapter.

Claude and Codex receive JSON containing `hookSpecificOutput`, including context, pre-tool permission decisions, and rewritten tool input where supplied. Kiro receives context as plain stdout; denying a `PreToolUse` event exits with code 2 and puts the reason on stderr. Input replacement is unsupported by the Kiro and Copilot adapters, so requesting a rewrite reports an error instead of silently dropping it. Other errors exit with code 1.

This entry point does not install or discover hooks. The agent still needs a registration that invokes the binary. A portable binary used as a Symposium-format plugin hook must be invoked with `--agent=symposium`.

## Hook formats

A plugin hook declares which wire format its handler expects:

- `format = "symposium"` (default) — the handler receives symposium canonical JSON. This is portable across all agents.
- `format = "antigravity"` / `"claude"` / `"copilot"` / `"codex"` / `"kiro"` — the handler receives that agent's native wire format.

## Dispatch rule

When symposium's global handler receives an event from agent A, it loads all plugins and finds hooks matching the event. For each plugin, it picks at most one hook to deliver:

1. If the plugin declares a hook with `format` matching **agent A** → deliver the input unmodified (the handler already expects this agent's native format).
2. Otherwise, if the plugin declares a **symposium-format** hook → convert to symposium canonical and deliver.
3. Otherwise → nothing fires for this plugin.

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

- Native format matching the host agent → pass through directly.
- Symposium format → convert to host agent's wire format.

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
