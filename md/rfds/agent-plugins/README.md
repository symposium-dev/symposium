<!-- See .agents/skills/authoring-rfds/SKILL.md for style guidance -->

# Agent Plugins interoperability

## TL;DR

- Compile each applicable Symposium plugin into an agent plugin directory and install that directory, instead of writing skill files into a different path for every agent.
- Read externally authored agent plugins as ordinary Symposium plugins, so a crate can publish a `plugin.json` at its root instead of a `SYMPOSIUM.toml`.
- Emit the Agent Plugins format for the agents that read it, Claude Code's own plugin format, and Gemini CLI's extension format, from a single compiled model.
- Write each directory once and register its path with an agent where the agent accepts one, copying only where it does not.
- Install through `cargo agents sync`, at the same scope as the enablement that selected the plugin. No new command.
- Cover the format's skills component in version one. MCP servers are a follow-up that belongs to the meta-server design.

## Motivation

Symposium installs skills with one mechanism per agent. A skill directory goes to `.claude/skills/` for one agent, `.agents/skills/` for five, and `.kiro/skills/` for another, each with a separate global path. Symposium carries that knowledge itself, and supporting a new agent means learning another placement rule.

The agents have since converged on a common answer. Every one of them that grew a plugin system settled on the same unit: a directory containing a manifest and a `skills/` folder. Three of the agents Symposium supports read the [Agent Plugins](https://agent-plugins.org/) format directly. Claude Code and Gemini CLI read formats of their own that differ mainly in the manifest filename. Producing that directory lets each agent's own loader perform the placement Symposium performs by hand.

A directory is also a better unit to own. Symposium writes a `.gitignore` containing `*` into each installed skill directory and marks each with a `.symposium` file so that it can later remove what it no longer owns. Applied per plugin rather than per skill, this becomes one hidden directory to write and one directory to remove when a plugin stops applying.

The reading direction is currently worse than absent. Registry enumeration claims a directory that holds a `SYMPOSIUM.toml` or a `SKILL.md` and otherwise descends into it. A registry of agent plugins therefore yields one claimed entry per `skills/<name>/SKILL.md`. The package name, its version, and its identity are discarded, and its skills arrive as unrelated dormant plugins.

The same content is treated three different ways depending on where it sits. A crate that ships a `plugin.json` beside a `skills/` directory already has its skills collected, because crate defaults scan `skills/` regardless of the manifest beside it; such a crate is missing only its metadata and validation. Registries and workspace members receive nothing at all.

Accepting the format is also consistent with how Symposium approaches distribution. The project routes plugin delivery through existing package registries rather than building its own. Letting a crate describe its extensions with a published open format is the same decision applied to packaging.

## Install contract

Symposium guarantees to an agent that it:

- selects the extensions relevant to the current workspace, evaluating every gate before anything is written;
- delivers the selected skills in the unit that agent understands;
- installs at the scope of the enablement that selected the plugin, and never wider;
- writes only into locations it marks as its own, leaving user-managed content untouched; and
- removes what it previously installed and no longer owns, on the next sync — whenever a plugin stops applying, not only when Symposium itself is removed.

What Symposium does not delegate is equally definite. Predicates and `depends-on` are evaluated before a directory is produced, so an agent never receives a gate and never resolves one. Hooks remain registered and dispatched by Symposium, which is what allows a hook to be authored once in a vendor-neutral format and evaluated per dispatch. Installations, custom predicates, and subcommands have no portable representation and stay where they are.

### Version one

Version one delivers:

- a compiled plugin directory produced from an already-gated Symposium plugin, carrying its manifest and its resolved skills;
- installation of that directory for the five agents that have a plugin unit, in each one's own dialect;
- recognition of an externally authored `plugin.json` package wherever a plugin is already found, and resolution of its skills;
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
  plugin.json
  .gitignore          contains *
  skills/
    extract-tables/
      SKILL.md
```

That directory is then installed for every agent that accepts one, in whichever manifest dialect the agent reads — see [Agent backends](#agent-backends) below for which format and mechanism each agent gets. The change is built around three replaceable boundaries: a compiled plugin model derived from an already-gated Symposium plugin; a per-agent emitter that renders that model into a directory and installs it; and a loader that recognizes an externally authored package as an ordinary plugin.

## Detailed plans

### Walkthrough

A user working on a Rust project wants the table-extraction guidance that a configured registry publishes as an agent plugin. Nothing about the steps is new; only what lands on disk changes.

```console
$ cargo agents search pdf
user-plugins
  pdf-tools    Table extraction guidance    agent plugin

$ cargo agents use pdf-tools
enabled pdf-tools for /home/alex/work/reporter
installed pdf-tools
  claude    registered   .symposium/plugins/pdf-tools
  codex     copied       .codex/plugins/pdf-tools
  gemini    skills only  .agents/skills/extract-tables
```

`use` records the enablement, workspace-scoped unless `--global` is given, and then syncs. The compiled directory is written once under `.symposium/plugins/` for a workspace-scoped plugin, or under the user configuration directory for a global one. Each agent is then given it by whichever mechanism it supports, which is why the three lines differ: Claude Code is pointed at the path, Codex CLI receives a copy, and Gemini CLI, which cannot express a project-scoped plugin, falls back to per-project skill directories.

```console
$ cargo agents status
active
  pdf-tools    agent plugin    used in this workspace
```

The agent is then started as usual and the skill is available. Whether it truly arrived is confirmed by asking the agent what it can see, since a file in a plausible location can still be ignored.

```console
$ cargo agents remove pdf-tools
disabled pdf-tools for /home/alex/work/reporter
removed pdf-tools from claude, codex, gemini
```

Two other routes reach the same place without naming a plugin. Adding a dependency whose source carries plugin content produces a consent question on the next sync, and answering yes installs it exactly as above. Placing a `plugin.json` beside a `skills/` directory in a workspace member requires no command at all, because workspace membership is itself the gate.

### Compiled plugin model

The compiled model is a mapping rather than a rename, because a Symposium plugin holds more than the shared format can express. Skill groups from any source, including `source.git` groups that must be fetched first, are resolved into a self-contained `skills/` directory. The plugin name and version become the manifest. Everything else either stays with Symposium or has already been consumed.

Gating is the significant part. Because predicates are evaluated before compilation, the emitted directory contains only what applies to the current workspace. Nothing is lost by the format's lack of an activation vocabulary, because the format never needs one.

### Installation

Symposium writes each compiled directory once, into a location it owns, and prefers to tell an agent where that directory is rather than copy it into the agent's own plugin folder. Registering a path keeps one copy per plugin instead of one copy per plugin per agent, and it is also what makes a project-scoped installation possible on an agent whose plugin folder is user-level.

Two mechanisms therefore exist, and which one applies is a property of the agent. VS Code accepts registered local plugin paths through a plugin-locations setting, and Claude Code accepts a local directory marketplace, so for those the directory stays where Symposium wrote it and the agent is pointed at it. Codex CLI and Kiro discover packages by their presence in a known folder, so for those the directory is copied there.

In both cases Symposium writes the agent's configuration itself rather than driving the agent's own plugin install command. This is what it already does for hook registration and MCP entries, and it is the only option on a path that runs during a hook, where there is no terminal and no user to answer a prompt. Because a well-formed file in a plausible location can still be ignored, the check that an installation worked is to ask the agent what it can see, not to inspect the file that was written.

Nothing about how a plugin is chosen changes. `cargo agents sync` compiles and installs whatever currently applies, `use` and `remove` record an enablement decision and then sync, and auto-sync performs the same work at session start. Installation remains a consequence of what applies to the workspace rather than a separate action, so this introduces no new command and no per-plugin install step.

### Scope of an installation

An installation is scoped to the enablement that produced it. A workspace-scoped `use` entry, a workspace member, and a workspace dependency all install for that project; a global entry installs for the user. This is the scoping `use` already has, applied to a different artifact.

Agents differ in whether they can honor it. Codex CLI, Kiro, and Claude Code each offer a project-scoped location. VS Code stores packages under a user-level path but accepts a workspace-scoped path registration, which is sufficient. An agent that offers only a user-level plugin folder and no way to register a path cannot express a project-scoped installation, and a workspace-scoped plugin must not become visible in unrelated projects. For such an agent, a workspace-scoped plugin continues to arrive through the existing per-project skills path, and only a global enablement is installed as a plugin. Gemini CLI is the likely case; which agents fall into it is confirmed before the emitters are written.

### Directory collisions

Several packages may sit side by side under one parent. Each is its own entry, which is what a registry already is, and no relationship between siblings is implied.

A `plugin.json` inside a directory that has already been claimed is ignored, because a claimed directory is not descended into. Nesting a package inside a package is therefore not a way to ship two, and the outer manifest is the one that describes the entry. A source root that is itself a package is an error, as it already is for the other two manifests: a source contains packages rather than being one.

On the install side, two plugins from different origins can resolve to the same directory name. The rule that already governs skill directories governs these: the plain name is used when the slot is free, and a name suffixed with a short hash of the package's origin is used when more than one origin claims the name or when a user-managed directory already occupies it. Cleanup keys on the ownership marker rather than on the shape of the name, so a package that moves between the two forms self-heals on the next sync.

### Agent backends

Each backend renders the same compiled model and installs the result by whichever mechanism its agent supports:

- GitHub Copilot and VS Code read the Agent Plugins format. The Copilot CLI stores packages under `~/.copilot/installed-plugins/`, and VS Code registers local plugin paths through a plugin-locations setting.
- Codex CLI reads the format as of v0.147.0, from `.codex/plugins/` for a project and `~/.codex/plugins/` for a user.
- Kiro reads the format as of v1.0.288, where a package is called a power and can be installed from a local folder.
- Claude Code reads its own near-identical format, a `.claude-plugin/plugin.json` beside `skills/`, `agents/`, and `hooks/`, installed through a local directory marketplace. A package installed this way is recorded as installed but not enabled, so the enablement entry is written as well.
- Gemini CLI reads extensions, a `gemini-extension.json` beside `skills/`, `commands/`, `hooks/`, and `agents/`, from `~/.gemini/extensions/<name>/`.

Claude Code and Gemini CLI are additional emitters over one model, not a separate design. Their directories carry the same resolved skills under a different manifest name.

OpenCode extends through TypeScript modules and Goose through MCP servers. Neither offers a directory-shaped plugin unit, so both retain the current per-skill install path. The existing mechanism is therefore reduced in scope rather than removed.

### External packages

Symposium recognizes two kinds of plugin directory today, one holding a `SYMPOSIUM.toml` and one holding a bare `SKILL.md`. A directory holding a `plugin.json` becomes a third. Precedence runs `SYMPOSIUM.toml`, then `plugin.json`, then `SKILL.md`. A directory carrying both a TOML and a JSON manifest loads as a Symposium plugin and takes its name and version from `plugin.json` where the TOML omits them.

Such a package is recognized in the three positions a plugin already occupies, and each position retains its existing meaning. A registry entry is curated and trusted. A workspace member is gated by membership. A dependency is an untrusted offer subject to consent.

Skills map without adaptation: `skills/` holds one skill per immediate child, in the `agentskills.io` format Symposium already uses, and deeper directories are not searched.

### Activation of external packages

The format cannot express when a package applies, so an externally authored package arrives without a gate. The existing enablement rules already resolve this. A plugin with nothing from which to infer a gate is dormant and is woken by a `use` entry; membership gates a workspace plugin; consent gates a dependency-embedded one. No new activation state is introduced, and `use`, `search`, and `status` describe these packages in the vocabulary they already use.

An author who wants a portable package to be dependency-scoped under Symposium declares `depends-on` or `predicates` beneath a `dev.symposium` key in the manifest's `extensions` object. The format requires a client to ignore namespaces it does not implement, and to do so without inspecting their contents, so the package remains portable.

### Failure containment

The format requires failures to be contained to the smallest affected unit and reported rather than suppressed, which matches how the report layer behaves. A manifest that violates its schema rejects that package alone and leaves the rest of the registry loading. An unrecognized top-level manifest field is reported and ignored. An invalid skill is skipped. A path that resolves outside the package directory is denied at the narrowest applicable boundary.

### Ordering

Compilation comes first because it changes how everything installs and is therefore the baseline the rest builds on. Accepting external packages adds a source of plugins, and that addition is considerably less valuable while those plugins would still be installed by the older per-skill mechanism.

## Frequently asked questions

### Which command installs a plugin?

None that is new. `cargo agents sync` compiles and installs whatever applies, `use` and `remove` record an enablement decision and then sync, and auto-sync does the same work at session start. A plugin is installed because it applies, not because it was installed by name.

### Why not drive each agent's own plugin install command?

Symposium already writes agent configuration directly for hooks and MCP entries, and using one mechanism keeps that consistent. An agent's install command also assumes a marketplace and a user at a terminal, neither of which is available on the auto-sync path, which runs inside a hook.

### Why register a path instead of copying into each agent's folder?

Copying reproduces the problem this change is meant to remove, one copy per agent, one level higher. Registration also decides scope: an agent whose plugin folder is user-level can still be given a project-scoped package if it accepts a path. Copying remains the fallback for agents that discover packages only by location.

### Why compile a directory rather than continue writing skill files directly?

Because the agents already implement a loader for that directory, and using it removes work instead of adding it. Symposium currently maintains a skills path per agent and per scope. Afterwards it maintains one fact per agent: how that agent is given a plugin directory.

### Is compilation lossy?

No. Predicates are evaluated before compilation, so an agent receives only what applies. Hooks, installations, custom predicates, and subcommands remain with Symposium and are dispatched by Symposium. What is handed over is the skills, which is precisely what is scattered today.

### Why does Claude Code need a separate emitter?

Its plugin format predates the standard and differs mainly in the manifest path and in supporting components the standard omits. Since the compiled model is unchanged, this is another rendering of the same content. Gemini CLI's extensions are the same case.

### What happens to OpenCode and Goose?

Neither has a plugin unit to target, so both continue to receive skills as individual directories. This is why the existing install path is narrowed rather than retired.

### Why `dev.symposium` for the extensions namespace?

The format asks a client to base its namespace on a domain it controls and to keep that namespace stable, since published identifiers should not move. `dev.symposium` corresponds to the organization the project publishes from, and control of the matching domain is confirmed before the identifier is published.

### Is the format stable enough to build on?

Version 1.0.0 was published in August 2026 by a committee spanning Amazon, Cursor, Microsoft, OpenAI, Vercel, and Google. Its schemas are versioned with the specification text, and published schema identifiers cannot be reassigned to different contents. The surface is two component types and a closed manifest, which bounds the cost of being wrong.

## Implementation plan

1. Confirm, for each agent, its plugin location and manifest dialect, its enablement mechanism, whether it accepts a registered path, and whether it can express a project-scoped installation. Confirm by observing what a running agent reports rather than by inspecting the files written to it.
2. Produce the compiled plugin directory from an already-gated plugin, validated against the published manifest schema, including the ownership marker, the hidden-directory rule, and name disambiguation on collision.
3. Install for the agents that read the Agent Plugins format, registering a path where one is accepted and copying where it is not, then retire their per-skill writes and remove the directories those writes left behind.
4. Add the Claude Code and Gemini CLI emitters over the same model, including the enablement entry Claude Code requires and the scope fallback for an agent that cannot express a project-scoped installation.
5. Recognize a `plugin.json` directory as a plugin entry in every position a plugin is already found, including the nesting and source-root rules.
6. Resolve an external package's skills, reporting an `mcp.json` as unsupported, and contain malformed packages, skills, and escaping paths at their proper boundaries.
7. Honor the `dev.symposium` extensions namespace, and extend `plugin validate`, `search`, `status`, and `use` to cover these packages.

### Future work

Emitting and reading `mcp.json` removes the last per-agent configuration format Symposium maintains, and follows the meta-server decision described above. It also introduces the format's subprocess contract, a `PLUGIN_ROOT` and a `PLUGIN_DATA` directory that survives updates, and requires a rule separating Symposium's own `${VAR}` expansion from the format's two placeholders.

Beyond that, Claude Code and Gemini CLI can both carry hooks inside a plugin directory, which would trade the vendor-neutral hook format and per-dispatch predicates for native dispatch. Publishing a compiled Symposium plugin as a portable package for other clients is a short step once compilation exists. A package's `version` can drive update checks and cache freshness, which the format explicitly permits.

## Implementation status

This RFD describes proposed work. Implementation has not begun.

See [Proposed: Agent Plugins packages](./proposed-reference.md) for the intended authoring reference and [Proposed: How extensions are installed](./proposed-install.md) for the resulting install locations.
