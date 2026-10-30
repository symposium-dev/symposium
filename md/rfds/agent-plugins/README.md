# Agent Plugins interoperability

## TL;DR

- Compile each applicable Symposium plugin into an agent plugin directory and install that directory, rather than writing skill files into a different path for each agent that has a plugin unit.
- Read externally authored agent plugins as ordinary Symposium plugins, so a crate can publish a `plugin.json` at its root instead of a `Symposium.toml`.
- Prefer the Agent Plugins format where an agent reads it, but do not treat it as the only way to register extensions: one compiled directory carries the Agent Plugins and Claude Code manifests at once, and the per-skill install path stays for the agents with no plugin unit, and for project-scoped plugins in version one.
- Write each directory once and register its path with an agent where the agent then reads it in place, copying where it does not. Only the Copilot CLI reads it in place, so most agents get a copy.
- Install through `cargo agents sync`, at the same scope as the enablement that selected the plugin. No new command.
- Cover the format's skills component in version one. MCP servers are a follow-up that belongs to the meta-server design.

## Motivation

Symposium installs skills with one mechanism per agent. A skill directory goes to `.claude/skills/` for one agent, `.agents/skills/` for five, and `.kiro/skills/` for another, each with a separate global path. Symposium carries that knowledge itself, and supporting a new agent means learning another placement rule.

The agents that grew a plugin system converged on the same unit, not on one format: a directory holding a manifest and a `skills/` folder. Codex CLI and the Copilot CLI read the [Agent Plugins](https://agent-plugins.org/) format itself, and Goose reads a root `plugin.json` of the same shape, which it calls Open Plugins. Claude Code and Antigravity CLI read formats of their own: Claude Code a manifest under `.claude-plugin/`, Antigravity CLI its own dialect of a root `plugin.json`. OpenCode has no plugin unit at all, and Kiro's has not been verified. Agent Plugins is therefore the format Symposium prefers to emit where an agent reads it rather than the single mechanism everything routes through, and the per-skill install path stays for the agents that cannot be given a directory. Either way, producing a directory lets the agent's own loader perform the placement Symposium performs by hand.

A directory is also a better unit to own. Symposium writes a `.gitignore` containing `*` into each installed skill directory and marks each with a `.symposium` file so that it can later remove what it no longer owns. Applied per plugin rather than per skill, this becomes one hidden directory to write and one directory to remove when a plugin stops applying.

The reading direction is currently worse than absent. Registry enumeration claims a directory that holds a `Symposium.toml` or a `SKILL.md` and otherwise descends into it. A registry of agent plugins therefore yields one claimed entry per `skills/<name>/SKILL.md`. The package name, its version, and its identity are discarded, and its skills arrive as unrelated dormant plugins.

The same content is treated three different ways depending on where it sits. A crate that ships a `plugin.json` beside a `skills/` directory already has its skills collected, because crate defaults scan `skills/` regardless of the manifest beside it; such a crate is missing only its metadata and validation. Registries and workspace members receive nothing at all.

Accepting the format is also consistent with how Symposium approaches distribution. The project routes plugin delivery through existing package registries rather than building its own. Letting a crate describe its extensions with a published open format is the same decision applied to packaging.

## Install contract

Symposium guarantees to an agent that it:

- selects the extensions relevant to the current workspace, evaluating every gate before anything is written;
- delivers the selected skills in the unit that agent understands;
- installs at the scope of the enablement that selected the plugin, and never wider;
- writes only into locations it marks as its own, leaving user-managed content untouched; and
- removes what it previously installed and no longer owns, on the next sync, whenever a plugin stops applying rather than only when Symposium itself is removed.

What Symposium does not delegate is equally definite. Predicates and `depends-on` are evaluated before a directory is produced, so an agent never receives a gate and never resolves one. Hooks remain registered and dispatched by Symposium, which is what allows a hook to be authored once in a vendor-neutral format and evaluated per dispatch. Installations, custom predicates, and subcommands have no portable representation and stay where they are.

### Version one

Version one delivers:

- a compiled plugin directory produced from an already-gated Symposium plugin, carrying its manifest and its resolved skills;
- installation of that directory at user scope for the agents that have a plugin unit (Claude Code, Codex CLI, the Copilot CLI, Antigravity CLI and Goose), carrying every manifest dialect at once;
- recognition of an externally authored `plugin.json` package wherever a plugin is already found, and resolution of its skills;
- optional `version` and `description` fields in every plugin manifest, which the compiled manifests carry;
- the `dev.symposium` extensions namespace, through which a portable package can carry Symposium gating; and
- coverage of these packages by `plugin validate`, `search`, `status`, and `use`.

Version one does not change:

- MCP server registration, which continues through the existing path;
- hook registration and dispatch, which remain Symposium's; or
- how plugins are distributed or enabled.

MCP servers are a follow-up rather than an omission. The format defines two component types and requires a conforming client to support at least one, naming a skills-only client as an example, so a directory with no `mcp.json` is valid and an incoming one is reported as unsupported. The reason to sequence it separately is that emitting an `mcp.json` makes each agent connect to every backing server itself and load all of their tool declarations at startup, which is the cost the [MCP meta-server](../mcp-meta-server/README.md) exists to avoid. That trade is a decision about the meta-server, and it is settled there before it is applied here. Skills raise no equivalent question, because a skill directory is the same content however it arrives.

## Change in a nutshell

Installing one plugin that carries one skill currently produces a separate write for each configured agent, each in a different location. In its place, Symposium compiles the plugin once:

```text
pdf-tools/
  plugin.json                     Codex CLI, Copilot CLI, Antigravity CLI, Goose
  .claude-plugin/plugin.json      Claude Code
  skills/
    extract-tables/
      SKILL.md
```

One directory serves every agent, because each agent's loader ignores the manifests that are not its own. That directory is then installed for every agent that accepts one; [Agent backends](#agent-backends) covers which mechanism each gets. The change is built around three replaceable boundaries: a compiled plugin model derived from an already-gated Symposium plugin; a set of manifest renderers and install mechanisms over that model; and a loader that recognizes an externally authored package as an ordinary plugin.

## Detailed plans

### Walkthrough

A user working on a Rust project wants the table-extraction guidance that a configured registry publishes as an agent plugin. Nothing about the steps is new; only what lands on disk changes.

```console
$ cargo agents search pdf
user-plugins
  pdf-tools    Table extraction guidance    agent plugin

$ cargo agents use pdf-tools --global
enabled pdf-tools for every workspace
installed pdf-tools
  claude        plugin   ~/.claude/skills/pdf-tools
  codex         plugin   ~/.codex/plugins/cache/symposium/pdf-tools
  antigravity   plugin   ~/.gemini/config/plugins/pdf-tools
```

`use` records the enablement, workspace-scoped unless `--global` is given, and then syncs. A global plugin is compiled once under `~/.symposium/installed/`, and each agent is then given it by whichever mechanism it supports, which is why the paths differ. Without `--global` the enablement would be workspace-scoped, and in version one every agent would receive the skill as an individual skill directory in the project, as it does today.

```console
$ cargo agents status
active
  pdf-tools    agent plugin    used in every workspace
```

The agent is then started as usual and the skill is available. Whether it truly arrived is confirmed by asking the agent what it can see, since a file in a plausible location can still be ignored.

```console
$ cargo agents use pdf-tools --remove --global
disabled pdf-tools for every workspace
removed pdf-tools from claude, codex, antigravity
```

Two other routes enable a plugin without naming it. Adding a dependency whose source carries plugin content produces a consent question on the next sync, and answering yes installs it for that project. Placing a `plugin.json` beside a `skills/` directory in a workspace member requires no command at all, because workspace membership is itself the gate.

### Compiled plugin model

The compiled model is a mapping rather than a rename, because a Symposium plugin holds more than the shared format can express. Skill groups from any source, including `source.git` groups that must be fetched first, are resolved into a self-contained `skills/` directory. The plugin name, normalized so that every agent accepts it, its version, `0.0.0` unless the plugin declares one, and its description become the manifest; since every agent reads either the directory in place or a copy Symposium rewrites, none needs the version to change to pick up an edit. Everything else either stays with Symposium or has already been consumed.

Gating is the significant part. Because predicates are evaluated before compilation, the emitted directory contains only what applies. Nothing is lost by the format's lack of an activation vocabulary, because the format never needs one.

### Installation

Symposium writes each compiled directory once, into a location it owns, and prefers to tell an agent where that directory is rather than copy it into the agent's own plugin folder. Registering a path keeps one copy per plugin instead of one copy per plugin per agent, so it is used wherever the agent then reads the directory in place.

Two mechanisms therefore exist, and which one applies is a property of the agent. The Copilot CLI is the one agent registered by path: its settings name the staging root as a directory marketplace and enable the plugin from it, and it reads the plugin where Symposium wrote it. Every other agent with a plugin unit gets a copy: Claude Code in its user skills directory, where it loads as a skills-directory plugin; Antigravity CLI and Goose in the folder each discovers plugins in; and Codex CLI in its own plugin cache, plus an enablement entry in its `config.toml`, because a Codex session loads a plugin only from that cache and Codex refreshes the cache itself only when the version changes. Claude Code could read the staging root as a directory marketplace instead, but it then keeps installation records of its own that a clean removal would have to edit. [Agent backends](#agent-backends) records which agent falls where, and what that was verified against.

In both cases Symposium writes the agent's configuration itself rather than driving the agent's own plugin install command. This is what it already does for hook registration and MCP entries, and it is the only option on a path that runs during a hook, where there is no terminal and no user to answer a prompt. Because a well-formed file in a plausible location can still be ignored, the check that an installation worked is to ask the agent what it can see, not to inspect the file that was written.

Nothing about how a plugin is chosen changes. `cargo agents sync` compiles and installs whatever currently applies, `use` and `use --remove` record an enablement decision and then sync, and auto-sync performs the same work at session start. Installation remains a consequence of what applies to the workspace rather than a separate action, so this introduces no new command and no per-plugin install step.

A plugin installed by the auto-sync in the session-start hook appears in the next session, since every agent that receives plugins and runs that hook has read its plugins by then; Claude Code (`/reload-plugins`), the Copilot CLI (`/restart`) and Antigravity CLI (`/skills reload`) can pick it up sooner. Goose runs no hook, but looks for plugins on every message.

### Scope of an installation

An installation is scoped to the enablement that produced it. A plugin named by a `use --global` entry installs for the user, whatever `hook-scope` says, and an agent that takes it stops receiving per-project copies of its skills. A workspace-scoped `use` entry, a registry plugin active by its own gate, a workspace member, and a workspace dependency all install for that project. This is the scoping `use` already has, applied to a different artifact.

A global plugin installs with all its skills, its dependency gates counted as satisfied: a user-level directory is visible from every workspace, so evaluating those gates in the workspace being synced would make what it carries depend on whichever workspace synced last.

Project-scoped plugins keep arriving through the per-skill path on every agent in version one; [Which agents can scope a plugin to a project](#which-agents-can-scope-a-plugin-to-a-project) records what each agent offers and why none is used yet.

### Directory collisions

Several packages may sit side by side under one parent. Each is its own entry, which is what a registry already is, and no relationship between siblings is implied.

A `plugin.json` inside a directory that has already been claimed is ignored, because a claimed directory is not descended into. Nesting a package inside a package is therefore not a way to ship two, and the outer manifest is the one that describes the entry. A source root that is itself a package is an error, as it already is for the other two manifests: a source contains packages rather than being one.

On the install side, two plugins from different origins can resolve to the same directory name. The rule that already governs skill directories governs these: the plain name is used when the slot is free, and a name suffixed with a short hash of the package's origin is used when more than one origin claims the name or when a user-managed directory already occupies it. Cleanup keys on the ownership marker rather than on the shape of the name, so a package that moves between the two forms self-heals on the next sync.

### Agent backends

There is one backend, not one per agent. It renders the compiled model into the manifests described in [Change in a nutshell](#change-in-a-nutshell) and installs the result by whichever mechanism the agent supports: pointing the agent at the directory where it was written, or copying it into a folder the agent reads. Claude Code's manifest is a renderer over that one model rather than a separate design.

Which agent gets which mechanism, at which path, and what each of those was verified against is recorded per agent in [Agent details](../../design/agent-details/README.md), and [Which agents can scope a plugin to a project](#which-agents-can-scope-a-plugin-to-a-project) covers the scoping each agent can express.

OpenCode extends through TypeScript modules and offers no directory-shaped plugin unit, so it retains the per-skill install path, as Kiro does until its powers are verified. Goose does offer one since 1.40: a root `plugin.json` beside a `skills/` folder, found by its presence in a folder Goose reads.

The per-skill path and plugin delivery meet in `.agents/skills/`, which Antigravity CLI, Codex CLI, the Copilot CLI, Goose and OpenCode all read. A global plugin's skills stay copied there while any configured agent reading it does not take the plugin, as with OpenCode, and the agents that took it then see each skill twice: Codex CLI and Antigravity CLI list both, while the Copilot CLI and Goose use the copy. This is a known limitation.

### External packages

Symposium recognizes two kinds of plugin directory today, one holding a `Symposium.toml` and one holding a bare `SKILL.md`. A directory holding a `plugin.json` becomes a third. Precedence runs `Symposium.toml`, then `plugin.json`, then `SKILL.md`. A directory carrying both a TOML and a JSON manifest loads as a Symposium plugin and takes its name, version and description from `plugin.json` where the TOML omits them, `version` and `description` being new optional fields of the Symposium manifest. A directory holding the `.symposium` marker file is Symposium's own output and is never read back as a source.

Such a package is recognized in the three positions a plugin already occupies, and each position retains its existing meaning. A registry entry is curated and trusted. A workspace member is gated by membership. A dependency is an untrusted offer subject to consent.

Skills map without adaptation: `skills/` holds one skill per immediate child, in the `agentskills.io` format Symposium already uses, and deeper directories are not searched.

### Activation of external packages

The format cannot express when a package applies, so an externally authored package arrives without a gate. The existing enablement rules already resolve this. A plugin with nothing from which to infer a gate is dormant and is woken by a `use` entry; membership gates a workspace plugin; consent gates a dependency-embedded one. No new activation state is introduced, and `use`, `search`, and `status` describe these packages in the vocabulary they already use.

An author who wants a portable package to be dependency-scoped under Symposium declares `depends-on` or `predicates` beneath a `dev.symposium` key in the manifest's `extensions` object. The format requires a client to ignore namespaces it does not implement, and to do so without inspecting their contents, so the package remains portable.

### Failure containment

The format requires failures to be contained to the smallest affected unit and reported rather than suppressed, which matches how the report layer behaves. A manifest that violates its schema rejects that package alone and leaves the rest of the registry loading. An unrecognized top-level manifest field is reported and ignored. An invalid skill is skipped. A path that resolves outside the package directory is denied at the narrowest applicable boundary, as a security point: a package supplies only what is inside its own folder. That covers the manifest, the `skills` folder and each `SKILL.md`, and also links inside a skill's content, which are skipped and reported, while the links of other skill sources are followed wherever they point ([Symlinks in skill content](../../reference/skill-definition.md#symlinks-in-skill-content)).

### Ordering

Compilation comes first because it changes how everything installs and is therefore the baseline the rest builds on. Global delivery goes with it rather than after it: user scope is the one scope every agent with a plugin unit takes as a plugin, so a compile step that handled workspace scope alone would leave the per-skill path carrying almost everything. Accepting external packages adds a source of plugins, and that addition is considerably less valuable while those plugins would still be installed by the older per-skill mechanism.

## Frequently asked questions

### Which command installs a plugin?

None that is new. `cargo agents sync` compiles and installs whatever applies, `use` and `use --remove` record an enablement decision and then sync, and auto-sync does the same work at session start. A plugin is installed because it applies, not because it was installed by name.

### Why not drive each agent's own plugin install command?

Symposium already writes agent configuration directly for hooks and MCP entries, and using one mechanism keeps that consistent. An agent's install command also assumes a marketplace and a user at a terminal, neither of which is available on the auto-sync path, which runs inside a hook.

### Why register a path instead of copying into each agent's folder?

Copying reproduces the problem this change is meant to remove, one copy per agent, one level higher. In practice registration is the exception: only the Copilot CLI then reads the directory in place. Codex CLI loads a plugin only from its own cache, Antigravity CLI and Goose discover plugins by their presence in a folder, and Claude Code's directory marketplace leaves records of its own behind, so copying is what most agents get.

### Why compile a directory rather than continue writing skill files directly?

Because the agents already implement a loader for that directory, and using it removes work instead of adding it. Symposium currently maintains a skills path per agent and per scope. Afterwards it maintains one fact per agent: how that agent is given a plugin directory.

### Is compilation lossy?

No. Predicates are evaluated before compilation, so an agent receives only what applies. Hooks, installations, custom predicates, and subcommands remain with Symposium and are dispatched by Symposium. What is handed over is the skills, which is precisely what is scattered today.

### Does Claude Code need a separate emitter?

No. Its plugin format predates the standard and differs mainly in the manifest path and in supporting components the standard omits, and Antigravity CLI's dialect of the root `plugin.json` is a similar case. Because each agent ignores the manifests that are not its own, one compiled directory can carry both at once, so these are manifest renderers over one model rather than emitters of their own. See [One directory, several manifests](#one-directory-several-manifests).

### What happens to OpenCode and Goose?

OpenCode has no plugin unit to target, so it continues to receive skills as individual directories, as Kiro does until its powers are verified. Goose has one, and receives a global plugin as a plugin directory, as Antigravity CLI does.

### Why `dev.symposium` for the extensions namespace?

The format asks a client to base its namespace on a domain it controls and to keep that namespace stable, since published identifiers should not move. `dev.symposium` corresponds to the organization the project publishes from, and control of the matching domain is confirmed before the identifier is published.

### Is the format stable enough to build on?

Version 1.0.0 was published in August 2026 by a committee spanning Amazon, Cursor, Microsoft, OpenAI, Vercel, and Google. Its schemas are versioned with the specification text, and published schema identifiers cannot be reassigned to different contents. The surface is two component types and a closed manifest, which bounds the cost of being wrong.

## Implementation plan

1. Confirm each agent's plugin location, manifest dialect, enablement mechanism, and the scoping it can express, by observing what a running agent reports rather than by inspecting the files written to it. Done for Claude Code, Codex CLI, the Copilot CLI, Antigravity CLI, and Goose; the results are in [Implementation details](#implementation-details) below. Kiro remains to be probed.
2. Compile each plugin named by a `use --global` entry, with all its skills, into a directory under `~/.symposium/installed/` that carries both manifests and the ownership marker and is renamed on collision, and install it for every agent that can take a plugin: Claude Code, Codex CLI, the Copilot CLI, Antigravity CLI and Goose ([#332](https://github.com/symposium-dev/symposium/pull/332), on top of [#333](https://github.com/symposium-dev/symposium/pull/333), which resolves each agent's relocated configuration directory).
3. Install workspace-scoped plugins for Claude Code and retire its per-skill copies ([#352](https://github.com/symposium-dev/symposium/pull/352), parked until [#353](https://github.com/symposium-dev/symposium/issues/353) is settled).
4. Recognize a `plugin.json` directory as a plugin entry in every position a plugin is already found, including the nesting and source-root rules ([#348](https://github.com/symposium-dev/symposium/pull/348)).
5. Resolve an external package's skills, reporting an `mcp.json` as unsupported, and contain malformed packages, skills, and escaping paths at their proper boundaries ([#348](https://github.com/symposium-dev/symposium/pull/348), and [#354](https://github.com/symposium-dev/symposium/pull/354) for links inside skill content).
6. Honor the `dev.symposium` extensions namespace, and extend `plugin validate`, `search`, `status`, and `use` to cover these packages ([#348](https://github.com/symposium-dev/symposium/pull/348)).

### Future work

Emitting and reading `mcp.json` removes the last per-agent configuration format Symposium maintains, and follows the meta-server decision described above. It also introduces the format's subprocess contract, a `PLUGIN_ROOT` and a `PLUGIN_DATA` directory that survives updates, and requires a rule separating Symposium's own `${VAR}` expansion from the format's two placeholders.

Project scope as a plugin for Codex CLI, the Copilot CLI, Antigravity CLI and Goose waits until the costs [Which agents can scope a plugin to a project](#which-agents-can-scope-a-plugin-to-a-project) records are settled, and Kiro until a running Kiro shows where installed powers live and whether it discovers one placed there.

Beyond that, Claude Code and Goose can both carry hooks inside a plugin directory, which would trade the vendor-neutral hook format and per-dispatch predicates for native dispatch. Publishing a compiled Symposium plugin as a portable package for other clients is a short step once compilation exists. A package's `version` can drive update checks and cache freshness, which the format explicitly permits.

## Implementation details

What probing the installed agents and mapping this proposal onto the current code turned up, last verified in October 2026 against Claude Code 2.1.284, Codex CLI 0.159.3, the Copilot CLI 1.0.89, Antigravity CLI 1.2.14 and Goose 1.53.0. The first point constrains the implementation; the others are the evidence for the sections above.

### Where a global compiled directory goes

[Installation](#installation) writes a global plugin's directory under the user configuration directory. That directory already holds one: `~/.symposium/plugins/` is the builtin `user-plugins` registry (`Symposium::registry_instances` in `src/config.rs`), a directory symposium *reads* entries from.

Compiling into it makes symposium read its own output. A compiled directory carries a `plugin.json`, which registry enumeration claims as an entry once [external packages](#external-packages) are recognized, so every global install reappears as a plugin of the `user-plugins` registry on the next load.

Global compiled directories go in a sibling instead:

```text
~/.symposium/plugins/     registry symposium reads
~/.symposium/installed/   compiled directories symposium writes
```

Workspace scope is unaffected (`<project root>/.symposium/plugins/`, for the project-scoped installation of step 4 of the [implementation plan](#implementation-plan)), and the whole `.symposium/` directory is symposium's own, so one `.gitignore` containing `*` sits at its root rather than one per compiled directory.

### One directory, several manifests

This is the evidence for the single directory [Change in a nutshell](#change-in-a-nutshell) shows. Claude Code ignores a `plugin.json` at a package root and falls back to the directory name for identity, Agent Plugins clients ignore `.claude-plugin/`, and Antigravity CLI and Goose read the same root `plugin.json`, Antigravity CLI as its own dialect. So one compiled directory carrying both manifests, with the same content in each, satisfies every one of them:

```text
pdf-tools/
  plugin.json                    Codex CLI, Copilot CLI, Antigravity CLI, Goose
  .claude-plugin/plugin.json     Claude Code
  skills/extract-tables/SKILL.md every one of them
```

Each agent loaded that directory; Codex CLI loads its skills only with the 1.0.0 `$schema`, which is the one emitted.

The staging root carries one manifest of its own, `.claude-plugin/marketplace.json`, for the Copilot CLI, which takes a *marketplace* (a directory of packages) plus a per-package enablement entry rather than a path to one package.

So there is one emitter with several manifest renderers, not one emitter per agent.

### Which agents can scope a plugin to a project

This is the evidence for [Scope of an installation](#scope-of-an-installation). Every agent with a plugin unit can scope a plugin to a project, each at a cost that keeps it out of version one:

| Agent | Result |
|---|---|
| Claude Code | a copy in `<project>/.claude/skills/<name>/`, loaded in a trusted folder but only by a session started in that directory ([#353](https://github.com/symposium-dev/symposium/issues/353)) |
| Codex CLI | a marketplace and an enablement entry in the project's `.codex/config.toml`, honored only in a trusted project; the plugin cache stays user-level and keyed by marketplace name, so each project needs a name of its own; the entry needs an absolute path, in a file teams may commit |
| Copilot CLI | the user-scope entries in the repository's `.github/copilot/settings.local.json`, read only at the git root and under a marketplace name other than the user-level one, which wins a clash; the file has to stay out of version control |
| Antigravity CLI | a copy in `<project>/.agents/plugins/<name>/`, a folder Goose reads too, so one writer has to serve both; interactive sessions load it only in a trusted folder |
| Goose | a copy in `<project>/.agents/plugins/<name>/`, read only from the working directory |

Kiro is not installed here and remains unconfirmed.

## Implementation status

Implementation is in progress; the [implementation plan](#implementation-plan) names the pull requests behind each step.

See [Proposed: Agent Plugins packages](./proposed-reference.md) for the intended authoring reference and [Proposed: How extensions are installed](./proposed-install.md) for the resulting install locations.
