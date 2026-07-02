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
- **Extensible.** Events are JSON lines, so new event kinds and fields can be
  added without breaking older readers.
- **Never breaks a hook.** Every recording path is best-effort — failures are
  logged and swallowed.

## Event log format

When enabled, events are appended as **JSON lines** to per-day files under
`~/.symposium/telemetry/`:

```
~/.symposium/telemetry/events-2026-06-23.jsonl
```

Each line is one [`TelemetryEvent`](./module-structure.md): an `at` timestamp
plus a kind-tagged payload (`EventKind`), e.g.

```json
{"at":"2026-06-23T17:58:13Z","kind":"session_start","session_id":"P1","agent":"claude","plugins":["tokio-plugin"]}
{"at":"2026-06-23T17:58:14Z","kind":"user_prompt","session_id":"P1"}
{"at":"2026-06-23T17:58:15Z","kind":"tool_use","session_id":"P1","tool":"Bash"}
```

Files older than `RETENTION_DAYS` (30) are rolled off — deleted on the next
`SessionStart`.

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

## What we deliberately do *not* record

No prompt text, no shell command lines, no file paths. "How many tries until the
agent got it right" cannot be measured reliably from hook events; the trustworthy
version is an explicit user rating, which a later workstream will add.
