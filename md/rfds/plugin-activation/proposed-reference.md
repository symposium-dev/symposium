# Proposed: reference changes

> Proposed reference changes for the [Plugin activation and consent](./README.md) RFD: how the affected pages in `md/reference/` would read once it lands. Only the changed parts are shown.

## Plugin definition: `activation`

A new row in the top-level fields table:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `activation` | string | no | `always`, `suggested`, or `off`. How the plugin is offered when it is not enabled by name. See [Activation](#activation). |

It replaces the note on dormant plugins with this section:

### Activation

`activation` decides what happens when a plugin's predicates hold in a workspace:

- `always`: the plugin is active.
- `suggested`: the plugin is offered. An interactive `cargo agents sync` asks whether to enable it, and your agent is told it is available. It is active once you accept it.
- `off`: the plugin is never offered. It is active only when a [`[plugins] use`](../../reference/configuration.md#plugins) entry names it.

```toml
name = "tokio-guidance"
depends-on = ["tokio"]
activation = "suggested"
```

If you leave it out, a plugin in a registry is `always` when it references a dependency (a `depends-on` list or `depends-on(...)` predicate at the plugin, `[[skills]]`, `[[hooks]]`, `[[mcp_servers]]` or `[[plugins]]` level) or has a custom predicate in its plugin-level `predicates`, and `off` otherwise. A bare `SKILL.md` in a registry cannot set `activation`, so it always gets the inferred value.

A plugin embedded in a crate is always offered to the crate's dependents for consent, because depending on a crate should not let its author add instructions to your agent without asking. In a crate's manifest the only value that changes anything is `off`: the plugin is not offered, and loads only through `use` or a `[[plugins]]` reference from another plugin.

`activation` does not apply while you develop the crate or project that defines the plugin (workspace membership activates it), or when another plugin loads it through a `[[plugins]]` reference.

## Configuration: `[plugins]`

The `auto-enable` and `disable` rows of the table:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto-enable` | array | `[]` | Plugins you accepted when they were offered: a dependency's plugin, or a registry plugin whose `activation` is `suggested`. Each entry is a plain name (every workspace) or `{ name = "...", workspace = "/path" }` (that workspace root only). `"*"` accepts every offer. |
| `disable` | array | `[]` | Plugins that must not run. Same entry forms as `auto-enable`. Takes precedence over `use` and `auto-enable`, including over `"*"`. Written by the consent prompt and by [`cargo agents disable`](#new-page-cargo-agents-disable). |

```toml
[plugins]
auto-enable = ["serde", { name = "tokio-guidance", workspace = "/home/me/service" }]
disable = [{ name = "noisy-crate", workspace = "/home/me/service" }]
use = ["standalone-plugin"]
```

An entry scoped to a workspace applies only while you work in that workspace root. An accepted plugin still follows its predicates: if you remove the dependency it was offered for, it stops applying, and it comes back without a question when you add the dependency again.

The paragraph after the example becomes:

`use` is what wakes a plugin whose `activation` is `off`, and it also enables a plugin whether or not any dependency references it. `auto-enable` is narrower: it accepts what is offered to you, a dependency's plugin or a suggested registry plugin.

## `cargo agents sync`: consent prompt

Before syncing, an interactive `cargo agents sync` asks about each plugin that is offered to this workspace and that you have not decided about yet. Two kinds of plugin are offered: plugins embedded in your dependencies, and registry plugins whose `activation` is `suggested`. Five answers:

- **Ask me later** (the default): records nothing. You are asked again next time.
- **Enable in this workspace**: recorded in `[plugins] auto-enable`, scoped to this workspace, and installed by this same sync.
- **Enable in every workspace**: recorded in `[plugins] auto-enable` without a scope.
- **Not in this workspace**: recorded in `[plugins] disable`, scoped to this workspace.
- **Never**: recorded in `[plugins] disable` without a scope.

Only explicit answers are recorded, so pressing Enter through the prompt never declines anything. Escape leaves the remaining questions undecided.

## `cargo agents use`

The paragraph on registry plugins becomes:

A registry plugin whose `activation` is `always` is enabled by configuration, since pointing config at a registry is the act of trusting its curation, so `use`-ing it is a no-op and reports as such. A registry plugin whose `activation` is `suggested` or `off` is enabled by a `use` entry.

## `cargo agents status`

The `dormant` and `candidate` rows of the state table:

| State | Meaning |
|-------|---------|
| `dormant` | Loaded but contributing nothing: a registry plugin whose `activation` is `off`, awaiting [`cargo agents use`](../../reference/cargo-agents-use.md), or one whose predicates don't currently hold. |
| `candidate` | Offered and awaiting consent: a plugin embedded in a dependency, or a registry plugin whose `activation` is `suggested`. These are exactly what an interactive [`cargo agents sync`](../../reference/cargo-agents-sync.md) asks about. |

## New page: `cargo agents disable`

Turn a plugin off.

```bash
cargo agents disable <name> [--global] [--remove]
```

Records a [`[plugins] disable`](../../reference/configuration.md#plugins) entry and runs a sync, so the plugin's skills are removed right away. A disabled plugin does not run, whatever else enables it: a registry, `auto-enable`, or `use`.

The entry applies to the current workspace. With `--global` it applies to every workspace.

### `--remove`

Removes the entry in the matching scope: without `--global` the one recorded for this workspace, with it the global one. Fails if no entry matched. This is also how you undo a "Not in this workspace" or "Never" answer given in the consent prompt.
