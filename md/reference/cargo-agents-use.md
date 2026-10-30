# `cargo agents use`

Enable a plugin by name, and sync it into the workspace immediately.

## Usage

```bash
cargo agents use <NAME> [--global]
cargo agents use <NAME> --remove [--global]
```

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Plugin or crate name to enable |
| `--global` | Enable for every workspace instead of just the current one, and install it for you rather than per project |
| `--remove` | Drop a previously recorded enablement instead of adding one |

## Behavior

`use` is the durable, by-name form of consent. It records a `use` entry in the
[`[plugins]` section](./configuration.md) of the user config:

```toml
[plugins]
use = [
  "everywhere-plugin",                                    # --global
  { name = "crate-a", workspace = "/home/me/my-project" }, # default
]
```

Then it runs a sync, so the plugin's skills install right away rather than
waiting for the next one. A `--global` entry installs the plugin once for you
on the agents that support it, and per project on the others; see
[`cargo agents sync`](./cargo-agents-sync.md#plugins-enabled-for-every-workspace).

Two things a `use` entry can enable:

- **A dependency's embedded plugin.** Depending on a crate means compiling its
  code, not letting its author inject agent context, so a dependency is not a
  trust root — its embedded plugin stays off until you say otherwise. `use` is
  the by-name way to say so (`[plugins] auto-enable` is the ahead-of-time way).
- **A dormant registry plugin.** A plugin whose manifest names no dependency
  has nothing to gate it on, so it loads dormant. A `use` entry naming it is
  what wakes it.

Anything a configured registry already offers under a `depends-on` gate is
enabled by configuration, since pointing config at a registry is the act of
trusting its curation. `use`-ing it is a no-op and reports as such, unless
`--global` asks to install it for you: then the entry is recorded.

Enablement is not activation: `use` only adds to what *may* run. The plugin's
own [predicates](./predicates.md) still decide when it applies.

The name must name a package before it is recorded: a registry plugin (matched
exactly), a workspace dependency, or a crate on crates.io (matched the way
cargo matches crate names, so `widget-lib` and `widget_lib` are the same).
When a registry plugin and a crate share the name, the registry plugin wins.
Otherwise the command fails with `no package named <NAME>` and writes nothing.
Use [`cargo agents search`](./cargo-agents-search.md) to find the right name.

### `--remove`

Removes the entry in the matching scope: without `--global` the entry recorded
for the current workspace, with it the unscoped one. A scope mismatch (or no
entry at all) is an error rather than a silent success. The sync that follows
reaps the plugin's installed skills.

## Example

```bash
cargo agents search widget      # find it
cargo agents use widget-skills  # enable it here
cargo agents status             # confirm why it is on
cargo agents use widget-skills --remove
```
