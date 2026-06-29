# Structured report layer

Commands produce user-facing output by emitting tracing events with a `report` field. A custom tracing layer (`ReportLayer`) intercepts these events and renders them in one of three modes depending on CLI flags.

## How it works

```
Command code                  Tracing infrastructure             User
─────────────                 ──────────────────────             ────
tracing::info!(           →   ReportLayer::on_event()      →   stdout/stderr/JSON
  report = %ReportEvent::SkillInstalled { ... }
)
```

1. Command code emits a tracing event at `info` (actions) or `debug` (decisions) level, carrying a single `report` field whose value is a `ReportEvent` formatted via `Display` (which serializes to JSON).
2. The `ReportLayer` checks: does this event have a `report` field? Is its level within `max_level`?
3. If yes, it deserializes the JSON string back into a `ReportEvent` and renders it based on mode.

## Modes

| Mode | CLI flags | Output target | Level filter |
|------|-----------|---------------|--------------|
| `Normal` | (none) | stdout | INFO only |
| `Verbose` | `-v` | stderr | INFO + DEBUG |
| `Json` | `--json` | buffered → stdout at exit | INFO (or DEBUG with `-v --json`) |

The layer is **always installed** — commands don't need to check whether reporting is active.

## Adding a report event to a new command

### Step 1: Add a variant to `ReportEvent`

In `src/report.rs`, add a new variant to the enum:

```rust
/// A frobnitz was reticulated.
FrobnitzReticulated {
    name: String,
    count: usize,
},
```

Rules for variants:
- Use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields
- Don't use a field named `kind` (conflicts with `#[serde(tag = "kind")]`)
- Keep fields simple (String, bool, usize, Option)

### Step 2: Add a `format_human` arm

In the `format_human()` method, add a rendering arm:

```rust
Self::FrobnitzReticulated { name, count } => {
    format!("✅ reticulated {name} ({count} nodes)")
}
```

Use emoji prefixes to match the existing style:
- `✅` — success/action taken
- `➖` — removal
- `⚠️ ` — warning
- `ℹ️ ` — informational
- `🟢` — already in place / no-op

### Step 3: Emit from command code

```rust
tracing::info!(
    report = %crate::report::ReportEvent::FrobnitzReticulated {
        name: frobnitz.name.clone(),
        count: frobnitz.nodes.len(),
    },
);
```

Use `tracing::info!` for actions the user should always see, `tracing::debug!` for decision-trace detail that only appears with `-v`.

## Level conventions

| Level | When to use | Visible in |
|-------|-------------|------------|
| `info` | Actions taken (installed, removed, validated) | Normal, Verbose, Json |
| `debug` | Decisions (plugin matched, skill skipped, directory searched) | Verbose only (or `-v --json`) |

## The `Info` and `Warning` variants

For messages that don't map to a specific structured event, use the generic variants:

```rust
tracing::info!(
    report = %crate::report::ReportEvent::Info {
        message: format!("scanning {} workspace dependencies", count),
    },
);
```

Prefer specific variants over `Info`/`Warning` when the data is structured — they produce better JSON output.

## Testing

The test harness (`symposium-testlib`) provides `sync_with_report()` which installs a scoped `Json`-mode layer and returns captured events. Tests assert on the JSON structure:

```rust
let events = ctx.sync_with_report(tracing::Level::DEBUG).await?;
let installed: Vec<&Value> = events
    .iter()
    .filter(|e| e["kind"] == "skill_installed")
    .collect();
assert!(!installed.is_empty());
```

## Architecture notes

- The `Display` impl on `ReportEvent` serializes to JSON — this is how the value passes through tracing's `%` formatter into the visitor
- The layer's visitor checks `record_debug` (not `record_str`) because `%` goes through the debug path
- Per-layer `EnvFilter`s ensure the report layer receives all events regardless of file log level
- The `ReportHandle` (returned alongside the layer) allows draining accumulated JSON after the command completes
