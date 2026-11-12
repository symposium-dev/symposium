# `symposium status`

Show which plugins are enabled for this workspace, and why.

## Usage

```bash
symposium status
```

Must be run from within a workspace, as [`symposium sync`](./symposium-sync.md) is.

## Behavior

Symposium separates two questions. *Enablement* asks whether a plugin may run
at all; [*activation predicates*](./predicates.md) ask when it applies.
`status` reports both, one line per plugin, each naming its **enablement
root** — so it answers "why is this here?" with "enabled via `serde`".

Each line is in one of four states:

| State | Meaning |
|-------|---------|
| `active` | Enabled and its predicates hold here. The root names the trust root: workspace membership, a configured registry, `[plugins] auto-enable`, or a `[plugins] use` entry. |
| `dormant` | Loaded but contributing nothing: a registry plugin awaiting [`symposium use`](./symposium-use.md), one whose predicates don't currently hold, a dependency named by `use` that offers no plugin, or a `use` entry naming no registry plugin, dependency or cached crate (a [`symposium sync`](./symposium-sync.md) downloads a crate that is not cached yet). |
| `candidate` | Discovered in a dependency and awaiting consent. These are exactly what an interactive [`symposium sync`](./symposium-sync.md) asks about. |
| `declined` | Recorded in `[plugins] disable` — the record of pruned plugins and declined discoveries. |

Discovery is cache-only, so a dependency whose source has not been fetched yet
is simply not listed as a candidate. Enabling it by name still works.

With `--json`, each line is emitted as a `plugin_status` event carrying
`name`, `state`, `root`, and — for a discovered dependency plugin — the
resolved `version`.

## Example

```bash
$ symposium status
✅ my-tool — workspace member
✅ serde-skills 1.0.0 — `[plugins] use`
💤 team-conventions — registry `user-plugins` (dormant: awaiting `symposium use`)
❓ widget-lib 0.3.1 — found via dependency `widget-lib`, awaiting consent (`symposium use widget-lib`)
➖ noisy-crate — declined (`[plugins] disable`)
```
