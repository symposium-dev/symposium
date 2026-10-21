# Configuration loading

## Directory resolution

User-wide paths are resolved using the [`directories`](https://crates.io/crates/directories) crate, which handles XDG Base Directory conventions automatically. If XDG environment variables are set, they are respected; otherwise paths fall back to `~/.symposium/`.

See the [configuration reference](../reference/configuration.md#directory-resolution) for the full resolution table.

## Config loading

The user config (`~/.symposium/config.toml`) is loaded once at startup into the `Symposium` struct. The file is deserialized into `RawConfig`, then validated into the runtime `Config` used by the rest of the code. If the file is missing or empty, defaults are used. If reading or parsing fails (other than a missing file), a warning is printed and defaults remain available to read-only callers, but `Symposium` retains the load error. Sync, hook registration, initialization, and config writes reject that context rather than treating its empty agent list as an uninstall request. Hook-triggered auto-sync logs the error and continues the hook without recording a successful sync. Repair the file and rerun the command to load it again; a valid empty agent list still permits deliberate cleanup.
