# Configuration loading

## Directory resolution

User-wide paths are resolved using the [`directories`](https://crates.io/crates/directories) crate, which handles XDG Base Directory conventions automatically. If XDG environment variables are set, they are respected; otherwise paths fall back to `~/.symposium/`.

See the [configuration reference](../reference/configuration.md#directory-resolution) for the full resolution table.

## Config merging

Both user (`~/.symposium/config.toml`) and project (`.symposium/config.toml`) configs are loaded and merged. Project settings override user settings field-by-field within the `[agent]` section. Plugin sources come from the user config only. Skills and workflows come from the project config only.
