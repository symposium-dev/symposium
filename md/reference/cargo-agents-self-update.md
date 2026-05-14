# `cargo agents self-update`

Update symposium to the latest version.

## Usage

```bash
cargo agents self-update
```

## Behavior

1. **Check for updates** — runs `cargo search` against the configured registry to find the latest published version of symposium. If the installed version is already current, prints a message and exits.

2. **Install** — runs `cargo install symposium --force` to build and install the latest version.

## Configuration

The `auto-update` key in `~/.symposium/config.toml` controls update behavior. It is also configurable during `cargo agents init`.

### `auto-update`

| Value | Behavior |
|-------|----------|
| `"on"` (default) | Check the registry at most once per 24 hours. When a newer version is found, automatically install it and re-execute the current command with the new binary. |
| `"warn"` | Check the registry at most once per 24 hours. Print a message when a newer version is available. During hook invocations, the nudge is included in the session-start hook's `additionalContext`. |
| `"off"` | Never check for updates. |

The 24-hour throttle is tracked in `~/.symposium/state.toml`. The check is skipped for `self-update` itself (which always checks unconditionally).

## State file

`~/.symposium/state.toml` tracks:

- `version` — the semver of the binary that last ran. Updated on every invocation. Future versions can use a version mismatch to trigger migrations.
- `last-update-check` — timestamp of the last registry query. Used to throttle checks to once per 24 hours.

## Examples

Manual update:

```bash
cargo agents self-update
```

Disable all update checks:

```toml
# ~/.symposium/config.toml
auto-update = "off"
```

Warn instead of auto-updating:

```toml
# ~/.symposium/config.toml
auto-update = "warn"
```
