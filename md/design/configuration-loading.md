# Configuration loading

## Directory resolution

User-wide paths are resolved using the [`directories`](https://crates.io/crates/directories) crate, which handles XDG Base Directory conventions automatically. If XDG environment variables are set, they are respected; otherwise paths fall back to `~/.symposium/`.

See the [configuration reference](../reference/configuration.md#directory-resolution) for the full resolution table.

## Config loading

The user config (`~/.symposium/config.toml`) is loaded once at startup into the `Symposium` struct. If the file is missing or empty, defaults are used. If parsing fails, a warning is printed and defaults are used.

## Installed source config

The registry-ready config model is parsed and preserved even while sync still
uses the legacy source pipeline. Installed sources live under `[installed]`:

```toml
[installed]
paths = ["/home/me/dev/plugin-source"]
git = ["https://github.com/my-org/plugin-source"]

[installed.crates]
symposium-recommendations = "1"
my-org-plugins = { git = "https://github.com/my-org/my-org-plugins" }
my-local-crate = { path = "/home/me/dev/my-local-crate" }
```

`[installed.crates]` mirrors Cargo dependency-table syntax. Values may be
version strings or inline dependency tables; unknown inline-table fields are
preserved so later source-resolution phases can hand the spec to Cargo instead
of maintaining a partial Cargo manifest parser. The source-acquisition layer
now has that Cargo probe path; sync will start consuming it when the resolved
source graph is wired in.

New configs include `symposium-recommendations = "1"` in
`[installed.crates]`. For compatibility during the migration, the legacy
`[defaults] symposium-recommendations` and `[[plugin-source]]` fields still
feed the current sync path through `Symposium::plugin_sources()`. New code that
needs the registry-ready model should use `Symposium::installed_sources()` or
`Symposium::installed_crates()`.

## Discovery policy

User-configured discovery policy is parsed under `[discovery.allow]` and
`[discovery.deny]`, with the scalar wildcard shorthand accepted at the
`discovery.allow` / `discovery.deny` keys:

```toml
[discovery]
allow = "*"

[discovery.deny]
crates = { unsafe-plugin = "*" }
paths = ["/tmp/untrusted"]
git = ["https://github.com/bad/*"]
```

Policy evaluation is intentionally not wired into sync yet. The parsed shape is
available for the resolved-source graph phase.

## Workspace source metadata

`WorkspaceDeps` caches both the workspace root and workspace member crates.
Member entries include local package paths, which lets `ResolvedSourceGraph`
add the workspace root and each member crate as `workspace()` provenance source
nodes before any skill installation happens. Older cache files without the
member list still deserialize; they simply repopulate the field on the next
metadata refresh.
