# `cargo agents hook`

Entry point invoked by the agent's hook system on session events.

## Flow

The payload is parsed first, and the agent's parser can declare it not an occurrence of the symposium event (`AgentHookInput::should_dispatch`; false for an Antigravity `PreInvocation` after the first of a turn). Such a payload is answered with the agent's neutral output and skips every step below.

1. **Auto-sync** (if enabled) — when `auto-sync = true` in the user config, runs [`cargo agents sync`](./sync-agent-flow.md) to ensure skills are current. The workspace root is resolved from the payload's `cwd` field; if the payload does not include a working directory, the process's current working directory is used as a fallback. Runs quietly and non-fatally — failures are logged but don't block hook dispatch.

   **`SessionStart` is the refresh point.** Because it fires once per agent session, it does the expensive work that other events skip: it bypasses the `Cargo.lock` freshness gate (so skills re-sync even when the workspace's dependencies are unchanged) and passes `UpdateLevel::Check` so git registries and `source.git` skill groups are re-fetched if their upstream moved. Every other event keeps the cheap, `Cargo.lock`-gated path with `UpdateLevel::None` (debounced) to avoid per-event network and `cargo metadata` cost. The registry refresh on `SessionStart` (`ensure_registries` with `Check`, decided in the binary entry point from the event) still honors each registry's `auto-update` toggle. `SessionStart` also runs `prewarm_hook_sources`, which *refreshes already-installed* hook binaries/scripts (the `cargo`/`github` sources backing plugin hooks) — refresh-only, so it never eagerly installs a tool a hook may never use; first install still happens lazily at dispatch.

2. **Built-in dispatch** — symposium's own handling, before plugin hooks. `PreToolUse` produces nothing. On `SessionStart` three fragments are computed independently and, when present, joined into one `additionalContext`:
   - **Discovery hint** — when the active workspace exposes plugin-vended subcommands (the same workspace-filtered set listed by [`cargo agents --help`](./subcommands.md#help-text-grouping)), a line suggesting the agent run `cargo agents --help` to find them. Computed independently of the update-check throttle, so it fires whenever there is something to discover.
   - **Consent hint**: the dependency plugins awaiting consent, telling the agent to point the user at `cargo agents sync` or `cargo agents use`.
   - **Update nudge** — when `auto-update = "warn"`, the 24-hour check throttle has elapsed, and the registry reports a newer version: a line suggesting `cargo agents self-update`.

   On `PostToolUse` and `UserPromptSubmit`, when auto-sync just re-synced because `Cargo.lock` changed, a generic notice says there may be new plugins and points at `cargo agents sync`. That is right after a tool call that changed the dependencies, or the user's next prompt when they changed them. `run_auto_sync` returns that as a bool, so an event with an unchanged `Cargo.lock` does no extra work. The notice is lost when the agent ignores context on that event (Antigravity after a tool call), or when the change is first seen on `PreToolUse` or `Stop`, which sync but carry no notice.

   Agents without hook registration (OpenCode, Goose) never receive this; for them the only discovery surface is `cargo agents --help` itself.

3. **Dispatch to plugin hooks** — for each enabled plugin that defines a hook handler for the incoming event:
   - **Select format**: for each plugin, pick the best hook to deliver (see [Hooks](./hooks.md) for priority rules). If the plugin has a hook matching the current agent's format, deliver the input unmodified. Otherwise deliver in symposium canonical format (or convert to the declared format if only one non-symposium hook exists).
   - **Acquire and run**:
     - Ensure any `requirements` for the hook are acquired (on-demand, best-effort).
     - Resolve the hook's `command` (a named installation reference or inline declaration) into a runnable form:
       - If the installation declares a `source`, acquire it (install / cache / clone) and resolve the `executable` / `script` against the cached location. Dispatch acquires with `UpdateLevel::None` — it serves the cache (git checks debounced) rather than hitting the network on every event. Freshness comes from the `SessionStart` prewarm (step 1), which re-acquires every applicable hook's source with `Check` once per session.
       - If no source, the `executable` / `script` is taken as a path on disk (relative paths resolve against the plugin directory, so a refreshed plugin-source repo updates these for free).
       - Run the installation's `install_commands` (post-source) before invoking the runnable.
       - Spawn `path args…` directly for `Exec`, or `sh path args…` for `Script`.
     - Pass the event JSON (in the selected format) on stdin to the plugin's hook.
     - Collect output from each handler.
     - Convert output back to the agent's wire format.
     - Merge results (e.g., allow/block decisions, output text) across all handlers. `merge` lets a later value replace an earlier one, so on `SessionStart`, `UserPromptSubmit` and `PostToolUse` a `JoinedContext` keeps every source's context (the builtin fragments first, then each hook) and writes the joined text back after each merge. `PreToolUse` and `Stop` are left out: there some agents keep context in the field that holds the decision.
     - Return the merged result to the agent.
   - **A hook that exits 2 blocks**: dispatch stops, and `blocked_response` turns its stderr into the reason. On `PreToolUse` that is a canonical deny, translated like any other so each agent receives it in the form it honors (Antigravity ignores exit codes, Copilot drops the reason of a failing hook). On other events the process exits 2 with the reason on stderr.

4. **Respond**: the agent's handler renders the final output as a `HookResponse` (stdout, stderr, exit code). Most agents read everything from stdout; Kiro, whose stdout is plain context, is told to deny a tool through exit 2.

Plugin hooks can respond to agent-specific events (e.g., `pre-tool-use`, `post-tool-use`, `user-prompt-submit` for Claude Code). The available events depend on which agent is in use.
