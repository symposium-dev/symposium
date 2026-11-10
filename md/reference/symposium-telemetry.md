# `symposium telemetry`

Manage opt-in, per-user usage telemetry. See the
[telemetry design chapter](../design/telemetry.md) for the event format and the
[`[telemetry]` configuration](./configuration.md#telemetry) for the underlying
config key.

Telemetry is **off by default**, **local-first**, and **never uploaded
automatically** — you share it yourself. The preference is also offered during
`symposium init`.

## Usage

```bash
symposium telemetry [status]      # whether enabled, where data lives, how much is stored (default)
symposium telemetry enable        # turn on collection (writes [telemetry] enabled = true)
symposium telemetry disable       # turn it off
symposium telemetry show [--count N]   # print recent events (JSON lines) for inspection
```

## Where the data lives

When recording is wired in, anonymous events will be appended as JSON lines to
per-day files under `~/.symposium/telemetry/` (e.g. `events-2026-06-23.jsonl`),
with files older than 30 days rolled off automatically. Events record counts
and coarse metadata only (session starts, prompts, tool names) — no prompt
text, command lines, or file paths.
