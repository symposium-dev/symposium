# `cargo agents status`

Show which plugins are enabled for this workspace, and why.

## Usage

```bash
cargo agents status
```

Must be run from within a workspace, as [`cargo agents sync`](./cargo-agents-sync.md) is.

## Behavior

Symposium separates two questions. *Enablement* asks whether a plugin may run
at all; [*activation predicates*](./predicates.md) ask when it applies.
`status` reports both, one line per plugin, each naming its **enablement
root** — so it answers "why is this here?" with "enabled via `serde`".

Each line is in one of five states:

| State | Meaning |
|-------|---------|
| `active` | Enabled and its predicates hold here. The root names the trust root: workspace membership, a configured registry, `[plugins] auto-enable`, or a `[plugins] use` entry. |
| `dormant` | Loaded but contributing nothing: a registry plugin awaiting [`cargo agents use`](./cargo-agents-use.md), one whose predicates don't currently hold, a dependency named by `use` that offers no plugin, or a `use` entry naming no registry plugin, dependency or cached crate (a [`cargo agents sync`](./cargo-agents-sync.md) downloads a crate that is not cached yet). |
| `candidate` | Discovered in a dependency and awaiting consent. These are exactly what an interactive [`cargo agents sync`](./cargo-agents-sync.md) asks about. |
| `declined` | Recorded in `[plugins] disable` — the record of pruned plugins and declined discoveries. |
| `revoked` | The [audits list](./audits.md) revokes this release or payload of a crate, for the reason shown. It is not loaded and not offered for consent, whatever `use` or `auto-enable` say. `disable` outranks it: a crate that is both shows as `declined`. |

The line of a discovered dependency plugin, or of a cached crate `use`d
without being a dependency, ends with what the
[audits list](./audits.md) says about it, in the words the
[consent prompt](./cargo-agents-sync.md#consent-prompt) uses:
``audited by the `symposium-audits` list``, `not audited`, or, when the list
audits the crate but this payload cannot be checked against an audit,
`not audited: ...` with the reason. A revoked line has no audit part, since
its root already says who revoked it and why, and neither has a declined
crate that is also revoked. Nor do lines that are not looked up: plugins from
a registry or the workspace (trust roots), a `use`d crate that is not cached
yet, a `use`d dependency that offers no plugin, and a declined name discovery
never saw. An audit changes nothing else: an audited candidate is still a
candidate.

Discovery is cache-only, so a dependency whose source has not been fetched yet
is simply not listed as a candidate. Enabling it by name still works.

A revoked crate that is only reached through another plugin's `[[plugins]]`
chain, not named by a dependency or a `use` entry, is not listed.

With `--json`, each line is emitted as a `plugin_status` event carrying
`name`, `state` (`active`, `dormant`, `candidate`, `declined` or `revoked`),
`root`, a `version` when one is known (the resolved version for a discovered
dependency plugin, the manifest's for the others), `audit` on every line whose
human form shows an audit part (exactly that text), and
`"format": "agent plugin"` for a plugin that is an
[Agent Plugins package](./agent-plugins.md), which the human output shows
after its name and version.

## Example

```bash
$ cargo agents status
✅ my-tool — workspace member
✅ serde-skills 1.0.0 — `[plugins] use` — not audited
💤 team-conventions — registry `user-plugins` (dormant: awaiting `cargo agents use`)
❓ widget-lib 0.3.1 — found via dependency `widget-lib`, awaiting consent (`cargo agents use widget-lib`) — not audited
➖ noisy-crate — declined (`[plugins] disable`)
✅ tidy-crate 2.4.0 — `[plugins] auto-enable` — audited by the `symposium-audits` list
⛔ risky-crate 2.0.1 — revoked by the `symposium-audits` list: it tells the agent to fetch and run remote code
```
