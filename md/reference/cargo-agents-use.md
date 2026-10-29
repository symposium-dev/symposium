# `cargo agents use`

Enable a plugin by name, and sync it into the workspace immediately.

## Usage

```bash
cargo agents use <NAME> [--pm <PM>] [--global]
cargo agents use <NAME> --remove [--pm <PM>] [--global]
```

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Plugin or crate name to enable |
| `--pm <PM>` | The package manager offering the plugin, when more than one does. With `--remove`, removes only that package manager's entry. |
| `--global` | Enable for every workspace instead of just the current one |
| `--remove` | Drop a previously recorded enablement instead of adding one |

## Behavior

`use` is the durable, by-name form of consent. It records a `use` entry in the
[`[plugins]` section](./configuration.md) of the user config:

```toml
[plugins]
use = [
  { pm = "user-plugins", name = "everywhere-plugin" },                     # --global
  { pm = "cargo", name = "crate-a", workspace = "/home/me/my-project" },   # default
]
```

The entry names the package manager as well as the plugin, so it means the
same plugin wherever it is read. An entry you write by hand may leave `pm`
out (see [`[plugins]`](./configuration.md#plugins)).

Then it runs a sync, so the plugin's skills install right away rather than
waiting for the next one.

Two things a `use` entry can enable:

- **A dependency's embedded plugin.** Depending on a crate means compiling its
  code, not letting its author inject agent context, so a dependency is not a
  trust root — its embedded plugin stays off until you say otherwise. `use` is
  the by-name way to say so (`[plugins] auto-enable` is the ahead-of-time way).
- **A dormant registry plugin.** A plugin whose manifest names no dependency
  has nothing to gate it on, so it loads dormant. A `use` entry naming it is
  what wakes it.

Anything a configured registry already offers under a `depends-on` gate is
enabled by configuration — pointing config at a registry is the act of
trusting its curation — so `use`-ing it is a no-op and reports as such.

Enablement is not activation: `use` only adds to what *may* run. The plugin's
own [predicates](./predicates.md) still decide when it applies.

The name must resolve to something before it is recorded, otherwise the
command errors and writes nothing. `use` looks in three places, in order, and
the first with a match answers:

1. plugins from a configured registry,
2. the workspace's dependencies,
3. the ecosystem package managers, checking their caches before searching (crates.io, for cargo).

So enabling a recommendation or a dependency never searches the network, and a
crate that happens to share a recommendation's name does not get in the way. To
name that crate instead, pass `--pm cargo`: `--pm` narrows every step to that
package manager.
When one place has several matches (two package managers with a dependency of
the same name, say), `use` lists them and asks for `--pm`. When none has it,
the command fails with `no package named <NAME>`. A registry plugin's name is
matched exactly, and a crate's the way cargo matches crate names, so
`widget-lib` and `widget_lib` are the same. Use
[`cargo agents search`](./cargo-agents-search.md) to find the right name.

### `--remove`

Removes the entry in the matching scope: without `--global` the entry recorded
for the current workspace, with it the unscoped one. With `--pm`, a bare entry
counts as the package manager its name resolves to. A scope mismatch (or no
entry at all) is an error rather than a silent success. The sync that follows
reaps the plugin's installed skills.

## Example

```bash
cargo agents search widget      # find it
cargo agents use widget-skills  # enable it here
cargo agents status             # confirm why it is on
cargo agents use widget-skills --remove
```
