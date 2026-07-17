# Telemetry

Telemetry is **opt-in**, **per-user** (not per-project), and **local-first**:
events are gathered into `~/.symposium/`, and uploading is a separate step the
user takes deliberately. The goal is to learn whether Symposium is actually
helping, while keeping the user in control of their data.

## Design principles

- **Opt-in, per-user.** Nothing is recorded unless the user enables it; the
  preference lives in the user-wide `config.toml`.
- **Local-first.** Events are written to a local log under `~/.symposium/`.
  Uploading is a separate, deliberate step.
- **Anonymous by construction.** Events record counts and coarse metadata only —
  no prompt text, command lines, or file paths.
- **Extensible.** Events are JSON lines and readers ignore unknown fields, so a
  new field is backward compatible as long as it carries `#[serde(default)]`. A
  new event *kind* read by an older binary deserializes to a catch-all `unknown`
  variant, so the line is kept rather than dropped (its payload is not retained).
- **Never breaks a hook.** Every recording path is best-effort — failures are
  logged and swallowed.

## Event log format

When enabled, events are appended as **JSON lines** to per-day files under
`~/.symposium/telemetry/`:

```
~/.symposium/telemetry/events-2026-06-23.jsonl
```

Each line is one [`TelemetryEvent`](./module-structure.md): an `at` timestamp
and an optional `session_id` on the envelope, then a kind-tagged payload
(`EventKind`), e.g.

```json
{"at":"2026-07-09T17:58:13Z","session_id":"P1","kind":"session_start","agent":"claude","crate_count":42}
{"at":"2026-07-09T17:58:13Z","session_id":"P1","kind":"plugin_activation","plugin":"async-plugin","crates":["tokio"]}
{"at":"2026-07-09T17:58:13Z","session_id":"P1","kind":"skill_activation","skill":"async-patterns","plugin":"async-plugin","crates":["tokio"]}
{"at":"2026-07-09T17:58:13Z","session_id":"P1","kind":"skill_activation","skill":"rust-general","plugin":"core-plugin"}
{"at":"2026-07-09T17:58:13Z","session_id":"P1","kind":"sync_run","installed":2,"reaped":0,"plugins_matched":2}
{"at":"2026-07-09T17:58:14Z","session_id":"P1","kind":"user_prompt"}
{"at":"2026-07-09T17:58:15Z","session_id":"P1","kind":"tool_use","tool":"Bash"}
{"at":"2026-07-09T17:58:16Z","session_id":"P1","kind":"hook_invocation","hook":"format-check","plugin":"async-plugin","duration_ms":37,"exit_code":0}
{"at":"2026-07-09T17:58:40Z","session_id":"P1","kind":"stop"}
```

The event kinds fall into three groups by what produces them:

- **Session lifecycle**, keyed off the agent's hook events: `session_start`
  (agent name, workspace crate count), `user_prompt`, `tool_use` (tool name
  only), and `stop` (end of a turn).
- **Sync activity**, captured once per session when symposium syncs skills:
  `plugin_activation` and `skill_activation` (which plugin or skill applied, and
  the witness crates that triggered it), plus `sync_run`. In `sync_run`,
  `installed` counts the skills this pass created or updated, so a steady-state
  sync reports 0; count `skill_activation` events for how many skills apply.
  `reaped` counts stale directories removed, and `plugins_matched` the plugins
  that applied.
- **Hook activity**: `hook_invocation` (which hook of which plugin ran, its
  duration, and its exit code — `null` if the hook was killed by a signal).

Not every agent produces every event. `stop` needs an agent that registers a Stop
hook, which today is Claude alone. Copilot supplies no `session_id`, so its
events cannot be grouped into a session. Goose and OpenCode register no hooks at
all (they are skills-only), so an opted-in user of those agents records nothing.
Any cross-agent comparison has a different denominator per agent.

A `crates` list is the set of workspace crates that satisfied the activation's
predicates. It is omitted when the gate was a wildcard or a non-crate predicate
(`env`, `shell`, `path`), so a skill that applies for a non-crate reason records
no `crates` key.

Files older than `RETENTION_DAYS` (30) are rolled off — deleted on the next
`SessionStart`, whether or not telemetry is currently enabled, so data recorded
before a user opted out still ages out.

## Configuration

```toml
[telemetry]
enabled = true
```

The preference is collected during `cargo agents init` ("Enable anonymous usage
telemetry?") and can be toggled later with `cargo agents telemetry enable` /
`disable`.

## The `telemetry` subcommand

- `cargo agents telemetry status` — whether enabled, where data lives, and how
  much is stored.
- `cargo agents telemetry enable` / `disable` — toggle the opt-in.
- `cargo agents telemetry show [--count N]` — print recent events for
  inspection (the data the user would share).

## Scope: symposium's own activity

Every activation, sync, and hook event is computed from symposium's own plugin
registry (the configured plugin sources), not by scanning an agent's installed
skill directory. A skill or plugin that symposium did not install (a
hand-authored agent skill, or one from another source) is never in that
registry, so it produces no telemetry. A symposium skill gated on a non-crate
predicate is still recorded, since symposium activated it, with an empty
`crates` field. The `user_prompt` and `tool_use` events count session activity
as a whole but carry no skill or plugin name, so they never attribute unrelated
work to symposium.

## What we deliberately do *not* record

No prompt text, no shell command lines, no file paths. "How many tries until the
agent got it right" cannot be measured reliably from hook events; the trustworthy
version is an explicit user rating, which a later workstream will add.
