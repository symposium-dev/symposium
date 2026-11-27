# `symposium use`

Enable a plugin by name, and sync it into the workspace immediately.

## Usage

```bash
symposium use <NAME> [--global]
symposium use <NAME> --remove [--global]
```

## Options

| Flag | Description |
|------|-------------|
| `<NAME>` | Plugin or crate name to enable |
| `--global` | Enable for every workspace instead of just the current one, and install it for you rather than per project |
| `--remove` | Drop a previously recorded enablement or consent answer instead of adding one |

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
[`symposium sync`](./symposium-sync.md#plugins-enabled-for-every-workspace).

Without `--global` the command must run inside a workspace. With it, it runs
anywhere: outside a workspace it installs the plugin for you right away on
every configured agent that keeps plugins at user level (Claude Code, Codex
CLI, GitHub Copilot, Antigravity CLI and Goose). Kiro and OpenCode read skills
from the project only, so nothing is installed for them there, and the output
says so in one line:

```text
not installed for Kiro, OpenCode: the next sync in a workspace installs the skills there
```

Its MCP servers are registered right away as well, in each configured
agent's user-level file. With `hook-scope = "project"` they belong in a
project, so the next sync in a workspace registers them there instead, and the
output says so:

```text
MCP servers not registered: with `hook-scope = "project"` the next sync in a workspace registers them there
```

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
Use [`symposium search`](./symposium-search.md) to find the right name.
The entry records the name the package goes by, so `symposium use widget_lib`
records `widget-lib`.

A crate the [audits list](./audits.md#revocations) revokes cannot be enabled:
`use` fails with the revocation and its reason (for example ``revoked by the
`symposium-audits` list: it tells the agent to fetch and run remote code``) and
writes nothing, since a revocation outranks every `use` entry. The check is
made against the release that would load. A name in `[plugins] disable` is
refused the same way, and the error points at `symposium use <NAME> --remove`,
which drops that entry. A `"*"` entry is the exception: remove it by hand.

### `--remove`

Removes the entry in the matching scope: without `--global` the entry recorded
for the current workspace, with it the unscoped one; an entry in the other
scope stays. The name is matched
the way `use` matches it, so either spelling of a crate removes its entry. The
sync that follows reaps the plugin's installed skills and unregisters the MCP
servers symposium registered for it, from your user-level agent files and this
workspace's, unless a plugin still enabled declares a server of the same name.
Outside a workspace, `--remove --global` removes what was installed for you,
MCP servers included.

It also forgets the answer the [consent prompt](./symposium-sync.md#consent-prompt)
recorded for the name, its `[plugins] auto-enable` or `[plugins] disable`
entry, with or without `--global`, since those entries apply in every
workspace. A registry plugin of the same name does not hide the answer about
the dependency, whose MCP servers are unregistered too. Unless another entry,
such as a `use` entry or `"*"`, still covers it, the dependency is then
undecided again, so the next interactive `symposium sync` asks about it. A
`"*"` entry is left alone. With no `use` entry in that scope and no
`auto-enable` or `disable` entry naming the plugin, the command is an error
rather than a silent success.

## Example

```bash
symposium search widget      # find it
symposium use widget-skills  # enable it here
symposium status             # confirm why it is on
symposium use widget-skills --remove
```
