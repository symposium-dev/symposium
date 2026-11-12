# Plugin activation and consent

## TL;DR

- Add a plugin-level `activation` field with three values: `always` activates when the plugin's predicates hold, `suggested` asks the user first, and `off` activates only through `use`.
- An omitted field keeps today's behavior: a registry plugin that references a dependency is `always`, one that references none is `off` (today's *dormant*), and a plugin embedded in a dependency is `suggested`.
- A plugin embedded in a dependency is always offered for consent, as today. Its author can only keep it out of the offer with `off`; `always` has no effect there.
- A suggested registry plugin goes through the consent flow that dependency plugins already use: the `cargo agents sync` prompt, the agent hints, and `status`.
- Consent answers can be recorded for one workspace or for all of them: `auto-enable` and `disable` accept the `{ name, workspace }` form that `use` already has.
- Add `cargo agents disable`, the counterpart of `use`, to turn a plugin off in one workspace or in all of them.
- Out of scope: skill content, a heavier unsafe tier, letting the agent record the user's answer, and configuring untrusted registries. See [Not in this RFD](#not-in-this-rfd).

## Motivation

The onboarding flow we want is: install Symposium, run `init`, open an agent in a Rust project, and have a base set of Rust skills working without further steps. More specialized plugins, such as async guidance for a workspace that uses tokio or performance tooling for a service, are offered, and the user accepts or declines them.

A registry plugin cannot express the second case today. It has two outcomes:

- It references a dependency somewhere in its manifest, so it activates whenever its predicates hold.
- It references none, so it is dormant until a `[plugins] use` entry names it.

There is no way to say "offer this and ask". The always-on case is also expressed indirectly: the recommendations entry under `rust/` declares `depends-on = ["*"]` to avoid going dormant, not to describe a dependency.

Consent is global. The `cargo agents sync` prompt records an approval in `auto-enable` and a decline in `disable`, both plain name lists. Declining a plugin in one workspace declines it in all of them. The [registry-centric RFD](../registry-centric-plugins/README.md#future-work) lists workspace-scoped `disable` as future work.

## Change in a nutshell

A registry entry that should be offered rather than installed declares it:

```toml
name = "tokio-guidance"
depends-on = ["tokio"]
activation = "suggested"

[[skills]]
source.path = "skills"
```

In a workspace that depends on tokio, nothing is installed yet. An interactive `cargo agents sync` asks:

```text
`tokio-guidance` from registry `symposium-recommendations` provides 1 skill group. Enable it?
> Ask me later
  Enable in this workspace
  Enable in every workspace
  Not in this workspace
  Never
```

"Enable in this workspace" records the answer for that workspace only, and the same sync installs the skills:

```toml
[plugins]
auto-enable = [{ name = "tokio-guidance", workspace = "/home/me/service" }]
```

While the plugin is undecided, the agent hints name it and tell the agent to point the user at `cargo agents sync`, as they do for dependency plugins today. `cargo agents status` lists it as `candidate`.

Nothing changes for a manifest without `activation`. The base Rust set keeps activating on its own.

## Detailed plans

### The `activation` field

`activation` is a top-level manifest field:

| Value | Meaning |
|-------|---------|
| `always` | Active whenever the plugin's predicates hold. |
| `suggested` | When the plugin's predicates hold, it is offered to the user. It is active once the user accepts it, or once a `use` entry names it. |
| `off` | Never offered. Active only when a `use` entry names it. |

When the field is omitted, the value is inferred as it is today:

| Where the plugin comes from | Inferred value |
|-----------------------------|----------------|
| A registry entry that references a dependency (a `depends-on` list or `depends-on(...)` predicate at the plugin, skill-group, hook, MCP-server or `[[plugins]]` level), or has a custom predicate in its plugin-level `predicates` | `always` |
| Any other registry entry | `off` |
| A dependency's embedded plugin | `suggested` |

A registry entry that is a bare `SKILL.md` cannot declare `activation`, so it always takes the inferred value from its frontmatter `depends-on` and `predicates`.

`activation` decides how a plugin is offered on its own. It does not apply in three cases, which keep their current behavior:

- **Workspace plugins.** A workspace member's plugin is active through membership. A crate's `SYMPOSIUM.toml` is read both as a workspace plugin, while you develop the crate, and as a dependency offer, while others depend on it. So the field is ignored in the first case rather than rejected.
- **Chained references.** An active plugin that names a crate with `[[plugins]] source.cargo` loads that crate without asking again: a registry vouches for what its plugins name, and consent given to a dependency plugin covers what it names. The reference is the decision, whatever the crate declares. A chained crate that is also a dependency of the workspace is still offered on its own through discovery, and declining it there stops it through the chain too.
- **`use`.** A `use` entry enables the plugin it names, whatever it declares.

`disable` keeps precedence over all of them. Today only dependency plugins honor it; [#290](https://github.com/symposium-dev/symposium/issues/290) tracks the other paths, and this RFD assumes that fix.

### Dependency plugins always ask

Registries and the workspace are trust roots. A dependency is not: depending on a crate should not let its author add instructions to the agent without asking. So a plugin embedded in a dependency is offered for consent whatever its manifest declares, which is today's behavior.

The one choice its author has is `off`: the plugin is not offered, and loads only through `cargo agents use <crate>` or a `[[plugins]]` reference from another plugin. `auto-enable`, including `"*"`, never activates an `off` plugin, since it accepts offers and an `off` plugin is never offered.

`always` has no effect in a crate's manifest: the plugin is still offered.

### Offering a suggested plugin

Discovery already classifies the plugins that dependencies offer: enabled by `use`, auto-enabled, declined, or candidate. It now also classifies the registry plugins whose activation is `suggested` and whose predicates hold in the workspace. The places that show candidates pick them up:

- the consent prompt in `cargo agents sync` asks about them, naming the registry instead of "Dependency `x` provides";
- the `SessionStart` hint and the notice sent when dependencies change mid-session cover them, with wording that no longer says the plugins come from dependencies;
- `cargo agents status` lists them as `candidate` with the registry as their root, instead of as dormant.

The hints keep telling the agent not to enable anything itself.

An accepted suggestion activates the way a `use`d dormant plugin does today. The names that `auto-enable` accepts in the workspace travel in the predicate context next to the `use` names, so skills, hooks, MCP servers, subcommands, and help agree on what is active.

`cargo agents use <name>` on a suggested registry plugin records a `use` entry, as it does for a dormant one today. It stays a no-op for an `always` plugin.

### Recording answers per workspace

`auto-enable` and `disable` accept the entry shape `use` already accepts: a plain name, which applies in every workspace, or `{ name, workspace }`, which applies only in that workspace root.

```toml
[plugins]
auto-enable = ["serde", { name = "tokio-guidance", workspace = "/home/me/service" }]
disable = [{ name = "noisy-crate", workspace = "/home/me/service" }]
```

The prompt's answers map onto these entries:

| Answer | Recorded |
|--------|----------|
| Ask me later (default) | nothing |
| Enable in this workspace | `auto-enable` entry scoped to the workspace |
| Enable in every workspace | plain `auto-enable` entry |
| Not in this workspace | `disable` entry scoped to the workspace |
| Never | plain `disable` entry |

Precedence is unchanged, applied to the entries that apply in the current workspace. A `disable` entry wins over everything. A `use` entry enables. An `auto-enable` entry accepts a candidate. `auto-enable = ["*"]` accepts every candidate, including suggested registry plugins, which makes it the "auto-confirm" setting.

Entries name plugins by bare name, as `use` entries do today. A registry plugin and a crate with the same name, such as `dial9`, share one entry: an answer about one covers the other.

An accepted plugin stays gated by its predicates. When the dependency behind it is removed, the plugin stops applying and the next sync removes its skills, as it does today. The record stays, so adding the dependency back reactivates the plugin without asking again.

### `cargo agents disable`

`cargo agents disable <name>` records a `disable` entry the way `cargo agents use <name>` records a `use` entry, then syncs so the plugin's skills are removed right away. It is how a user turns off an `always` plugin, or declines a suggestion without going through the prompt.

```bash
cargo agents disable tokio-guidance            # this workspace
cargo agents disable tokio-guidance --global   # every workspace
cargo agents disable tokio-guidance --remove   # drop the entry for this workspace
```

It mirrors `use`: the entry is scoped to the current workspace unless `--global` is given, and `--remove` drops the entry in the matching scope, failing when none matched. `--remove` is also how a "Not in this workspace" or "Never" answer from the prompt is undone. The name is recorded as given, so a plugin can be disabled before it is ever offered.

### Not in this RFD

- **Content.** The base Rust skill set, an `unsafe-code-review` skill, and future suggestions (async guidance, battery packs, performance tooling with dial9, blessed.rs) are recommendations content. This RFD provides the mechanism they use.
- **A heavier unsafe tier.** A plugin gated on the workspace actually using `unsafe`, running Miri after changes, needs an "uses unsafe" predicate, which does not exist (`shell(...)` can approximate it). The `Stop` hook event it would run on is only delivered by Claude Code and Antigravity.
- **The agent recording the user's answer.** The hints keep pointing the user at `cargo agents sync`. Letting the agent run `use` or record a decline on the user's behalf is a separate decision.
- **Configuring untrusted registries.** See [How would an untrusted registry work?](#how-would-an-untrusted-registry-work)

Proposed changes to the user-facing reference are in [proposed-reference.md](./proposed-reference.md).

## Frequently asked questions

### Why a field instead of `depends-on = ["*"]` and dormancy?

Both rules infer intent from dependency references. `depends-on = ["*"]` means "any workspace" as a predicate, and is used as a way to stay out of dormancy. Neither can express "ask first". A field states the intent directly, and adds the value that is missing.

### Why infer the default instead of defaulting to `always`?

Every existing manifest keeps its behavior, so no registry has to change when this lands. It also keeps the [Agent Plugins RFD](../agent-plugins/README.md#activation-of-external-packages) valid: an externally authored package has no way to declare a gate, and stays dormant in a registry until it is `use`d.

### How would an untrusted registry work?

The dependency rule would generalize: a registry configured as untrusted would treat `always` as `suggested`, so its plugins are offered rather than installed. Unlike in a crate, `always` keeps a meaning there, because the same registry can be trusted by one user and not by another; it is honored where the user trusts the source. Configuring an untrusted registry is not part of this RFD.

### Why record an acceptance in `auto-enable` instead of `use`?

A `use` entry loads what it names whether or not anything offers it: a name no registry provides is loaded as a crate. If a registry later dropped or renamed a suggested entry, a `use` record would pull a crate with the same name without asking. `auto-enable` only accepts what is offered, and it is the record the prompt already writes.

### Why can `auto-enable` be scoped now?

The [discovery-sync RFD](../registry-centric-plugins/discovery-sync/README.md#scope) keeps it global because consenting to a dependency's plugin is a judgment about its author, not about a project. A suggestion is different: whether a workspace wants tokio guidance is a judgment about that workspace. A plain entry keeps the old meaning, so scoping is only added, not imposed.

### Should the base Rust set be `always` or `suggested`?

Open. The onboarding story asks the user before installing the base set. Keeping it `always` means it works with no prompt. Both are possible with this design, and the choice is one line in the recommendations repository. That line has to wait for a release that parses the field: manifests reject unknown fields, so older clients would skip the entry.

### Can the agent ask the user and record the answer?

Not in this RFD. Suggestions come from a trusted registry, so letting the agent run `cargo agents use` after asking in the chat is plausible. It changes what the hints instruct, though, and dependency plugins must keep the stricter rule. It can be added once the mechanism exists.

## Implementation plan and status

### Step 1: Make activation explicit

Parse `activation` in the SDK manifest schema and replace the inferred dormancy flag with it, applying the inference rules. `always` and `off` behave exactly like today's active and dormant plugins, and a dependency's plugin stays a consent candidate unless it declares `off`. A crate reached both through enablement and through a chained reference must still load through the chain when enablement rejects it as `off`. `plugin validate` keeps warning about an inferred `off`, now suggesting an explicit `activation = "off"` or a gate.

Tests: inference for gated, gateless, and dependency plugins; an explicit value overriding the inference; a dependency declaring `always` still offered; a dependency declaring `off` not offered, not activated by `auto-enable = ["*"]`, but loaded by `use`; a workspace plugin ignoring the field; a chained crate declaring `off` loading, also when `auto-enable = ["*"]` is set.

- [ ] Not started

### Step 2: Offer suggested registry plugins

Classify the suggested registry plugins whose predicates hold as candidates. Their predicates are evaluated where the active set is built, the one place with a full predicate context (custom predicates included). Add the accepted names to the predicate context. Update the prompt, the `SessionStart` hint, the mid-session notice, and `status` as described above. `use` records an entry for them.

Tests: a suggested plugin is a candidate only when its predicates hold; accepting installs its skills, hooks, and MCP servers; declining keeps it out; `use` enables it; `auto-enable = ["*"]` accepts it; `status` lists it once, as `candidate`.

- [ ] Not started

### Step 3: Record consent per workspace

`auto-enable` and `disable` accept `{ name, workspace }` entries. The prompt offers the five answers above and records scoped entries. Update the [scope section](../registry-centric-plugins/discovery-sync/README.md#scope) of the discovery-sync RFD and the "Turning plugins off" and "Future work" sections of the registry-centric RFD.

Tests: a scoped acceptance applies only in its workspace; a scoped decline only in its workspace; "Never" applies everywhere; a global `disable` wins over a scoped `auto-enable`; removing a dependency deactivates an accepted plugin and adding it back reactivates it without a prompt.

- [ ] Not started

### Step 4: Add `cargo agents disable`

Add the command, with `--global` and `--remove`, recording through the same config entries as the prompt and syncing afterwards. Reserve `disable` as a built-in subcommand name, list it in help, and point `use`'s refusal of a disabled name at `cargo agents disable --remove`. Turning off plugins other than dependency plugins relies on [#290](https://github.com/symposium-dev/symposium/issues/290).

Tests: disabling an `always` registry plugin removes its skills from the workspace; a workspace-scoped entry leaves other workspaces alone; `--global` applies everywhere; `--remove` restores the plugin, and fails when nothing matched.

- [ ] Not started
