# `cargo agents status`

Show which plugins are enabled for this workspace, and why.

## Usage

```bash
cargo agents status
```

Must be run from within a Rust workspace.

## Behavior

Symposium separates two questions. *Enablement* asks whether a plugin may run
at all; [*activation predicates*](./predicates.md) ask when it applies.
`status` reports both, one line per plugin, each naming its **enablement
root** — so it answers "why is this here?" with "enabled via `serde`".

Each line is in one of four states:

| State | Meaning |
|-------|---------|
| `active` | Enabled and its predicates hold here. The root names the trust root: workspace membership, a configured registry, `[plugins] auto-enable`, or a `[plugins] use` entry. |
| `dormant` | Loaded but contributing nothing: a registry plugin awaiting [`cargo agents use`](./cargo-agents-use.md), one whose predicates don't currently hold, or a dependency named by `use` that offers no plugin. |
| `candidate` | Discovered in a dependency and awaiting consent. These are exactly what an interactive [`cargo agents sync`](./cargo-agents-sync.md) asks about. |
| `declined` | Recorded in `[plugins] disable` — the record of pruned plugins and declined discoveries. |

Discovery is cache-only, so a dependency whose source has not been fetched yet
is simply not listed as a candidate. Enabling it by name still works.

With `--json`, each line is emitted as a `plugin_status` event carrying
`name`, `state`, `root`, a `version` when one is known (the resolved version
for a discovered dependency plugin, the manifest's for the others), and
`"format": "agent plugin"` for a registry or workspace plugin that is an
[Agent Plugins package](./agent-plugins.md), which the human output shows
after its name and version.

## Example

```bash
$ cargo agents status
✅ my-tool — workspace member
✅ serde-skills 1.0.0 — `[plugins] use`
💤 team-conventions — registry `user-plugins` (dormant: awaiting `cargo agents use`)
❓ widget-lib 0.3.1 — found via dependency `widget-lib`, awaiting consent (`cargo agents use widget-lib`)
➖ noisy-crate — declined (`[plugins] disable`)
```
