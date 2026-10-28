# Configuration loading

## Directory resolution

User-wide paths are resolved from environment variables in `dirs.rs` (config and cache) and `config.rs` (`resolve_logs_dir`): `SYMPOSIUM_HOME` first, then the matching `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` or `XDG_STATE_HOME`, then the default (`~/.symposium/`, with `cache/` and `logs/` under the config directory). An empty variable counts as unset (`dirs::env_dir`), and a relative `XDG_*` value is ignored as the XDG Base Directory spec asks (`dirs::xdg_dir`), so neither resolves against the current directory.

See the [configuration reference](../reference/configuration.md#directory-resolution) for the full resolution table.

## Config loading

The user config (`~/.symposium/config.toml`) is loaded once at startup into the `Symposium` struct. The file is deserialized into `RawConfig`, then validated into the runtime `Config` used by the rest of the code. If the file is missing or empty, defaults are used.

If the file exists but cannot be read or parsed, a warning is printed and the defaults are used for that run only, with `auto-update` forced off. The error is kept on `Symposium` (`config_error`), and while it is set `save_config` refuses to write: a command that would save the config (`init`, `use`, `telemetry enable|disable`, the consent prompt in `sync`) fails with an error naming the file and leaves it unchanged.

A file Symposium could not read says nothing about what the user chose, so nothing decided from the defaults may outlive the run. Auto-update is forced off because it is the one default that acts outside Symposium's own files; the rest apply as usual. The failure is not fatal, since hooks run on every agent event and must keep working.

Sync, hook registration, and initialization also reject that context rather than treating its empty agent list as an uninstall request. Hook-triggered auto-sync logs the error and continues the hook without recording a successful sync. A dangling config symlink is a read failure, not a missing file. Repair the file and rerun the command; a valid empty agent list still permits deliberate cleanup.
