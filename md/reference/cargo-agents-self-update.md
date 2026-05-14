# `cargo agents self-update`

Update symposium to the latest version.

## Usage

```bash
cargo agents self-update
```

## Behavior

1. **Check for updates** — runs `cargo search` against the configured registry to find the latest published version of symposium. If the installed version is already current, prints a message and exits.

2. **Download** — by default (`self-update-source = "binary"`), downloads a prebuilt binary from the GitHub release for the current platform. If the download fails (e.g., no prebuilt binary for the platform), falls back to `cargo install`.

3. **Install** — extracts the binary and atomically replaces the installed copy in `$CARGO_HOME/bin/` (or `~/.cargo/bin/`). When using the source fallback, runs `cargo install symposium --force`.

### Supported platforms (prebuilt binaries)

| Target | Archive format |
|--------|---------------|
| `aarch64-apple-darwin` | `.tar.gz` |
| `x86_64-unknown-linux-musl` | `.tar.gz` |
| `aarch64-unknown-linux-musl` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `.zip` |

Other platforms can use `self-update-source = "source"` to build from source.

## Configuration

Two keys in `~/.symposium/config.toml` control update behavior:

### `auto-update`

| Value | Behavior |
|-------|----------|
| `"off"` | Never check for updates. |
| `"warn"` (default) | Check the registry at most once per 24 hours. Print a message when a newer version is available. |
| `"on"` | Check the registry at most once per 24 hours. When a newer version is found, automatically download it and re-execute the current command with the new binary. |

The 24-hour throttle is tracked in `~/.symposium/state.toml`. The check is skipped for hook invocations and for `self-update` itself (which always checks unconditionally).

### `self-update-source`

| Value | Behavior |
|-------|----------|
| `"binary"` (default) | Download a prebuilt binary from the GitHub release. Falls back to `cargo install` if the download fails. |
| `"source"` | Build from source via `cargo install symposium --force`. |

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

Auto-update on every invocation (within the 24-hour window):

```toml
# ~/.symposium/config.toml
auto-update = "on"
```

Force building from source:

```toml
# ~/.symposium/config.toml
self-update-source = "source"
```
