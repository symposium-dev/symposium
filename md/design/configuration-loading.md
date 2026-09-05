# Configuration loading

## Directory resolution

User-wide paths are resolved using the [`directories`](https://crates.io/crates/directories) crate, which handles XDG Base Directory conventions automatically. If XDG environment variables are set, they are respected; otherwise paths fall back to `~/.symposium/`.

See the [configuration reference](../reference/configuration.md#directory-resolution) for the full resolution table.

## Config loading

The user config (`~/.symposium/config.toml`) is loaded once at startup into the `Symposium` struct. The file is deserialized into `RawConfig`, then validated into the runtime `Config` used by the rest of the code. If the file is missing or empty, defaults are used. If parsing fails, a warning is printed and defaults are used.

That last rule sets the cost of `deny_unknown_fields`, which most sections carry: one misspelled key does not fall back to the default for *that key*, it falls back to the default for the *whole config* — agents, registries and hook scope included. It is the right trade for a typo (a silently ignored setting is worse), but it means removing a key the code once accepted is a breaking change for anyone still naming it. `[experiments]` is where that bites, since experiments are expected to disappear: a graduated flag stays accepted-and-ignored for a release rather than being deleted.
