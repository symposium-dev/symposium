# Agent Plugins interoperability

## TL;DR

- Compile each applicable Symposium plugin into an agent plugin directory and install that directory, rather than writing skill files into a different path for each agent that has a plugin unit.
- Read externally authored agent plugins as ordinary Symposium plugins, so a crate can publish a `plugin.json` at its root instead of a `Symposium.toml`.
- Prefer the Agent Plugins format where an agent reads it, but do not treat it as the only way to register extensions: one compiled directory carries the Agent Plugins and Claude Code manifests at once, and the per-skill install path stays for the agents with no verified plugin unit, and for project-scoped plugins on every agent except Claude Code.
- Write each directory once and register its path with an agent where the agent then reads it in place, copying where it does not. Only the Copilot CLI is registered by path; Claude Code, Codex CLI, Antigravity CLI and Goose get a copy.
- Install through `cargo agents sync`, at the same scope as the enablement that selected the plugin: a `use --global` entry installs for the user, and anything else installs for the project. No new command.
- Cover the format's skills component in version one. MCP servers are a follow-up that belongs to the meta-server design.

## Motivation

Symposium installs skills with one mechanism per agent. A skill directory goes to `.claude/skills/` for one agent, `.agents/skills/` for five, and `.kiro/skills/` for another, each with a separate global path. Symposium carries that knowledge itself, and supporting a new agent means learning another placement rule.

The agents that grew a plugin system converged on the same unit, not on one format: a directory holding a manifest and a `skills/` folder. Codex CLI and the Copilot CLI read the [Agent Plugins](https://agent-plugins.org/) format itself, and Goose reads a root `plugin.json` of the same shape, a format it calls Open Plugins. Claude Code and Antigravity CLI read formats of their own: Claude Code a manifest under `.claude-plugin/`, Antigravity CLI its own dialect of a root `plugin.json`. OpenCode has no plugin unit at all, and Kiro's has not been verified. Agent Plugins is therefore the format Symposium prefers to emit where an agent reads it rather than the single mechanism everything routes through, and the per-skill install path stays for the agents that cannot be given a directory. Either way, producing a directory lets the agent's own loader perform the placement Symposium performs by hand.

A directory is also a better unit to own. Symposium writes a `.gitignore` containing `*` into each installed skill directory and marks each with a `.symposium` file so that it can later remove what it no longer owns. Applied per plugin rather than per skill, this becomes one hidden directory to write and one directory to remove when a plugin stops applying.

The reading direction is currently worse than absent. Registry enumeration claims a directory that holds a `Symposium.toml` or a `SKILL.md` and otherwise descends into it. A registry of agent plugins therefore yields one claimed entry per `skills/<name>/SKILL.md`. The package name, its version, and its identity are discarded, and its skills arrive as unrelated dormant plugins.

The same content is treated three different ways depending on where it sits. A crate that ships a `plugin.json` beside a `skills/` directory already has its skills collected, because crate defaults scan `skills/` regardless of the manifest beside it; such a crate is missing only its metadata and validation. Registries and workspace members receive nothing at all.

Accepting the format is also consistent with how Symposium approaches distribution. The project routes plugin delivery through existing package registries rather than building its own. Letting a crate describe its extensions with a published open format is the same decision applied to packaging.

## Install contract

Symposium guarantees to an agent that it:

- selects the extensions relevant to the current workspace, evaluating every gate before anything is written;
- delivers the selected skills in the unit that agent understands;
- installs at the scope of the enablement that selected the plugin, and never wider;
- writes only into locations it marks as its own and into configuration entries it owns by name, leaving user-managed content untouched, including a plugin the user has turned off in the agent; and
- removes what it previously installed and no longer owns, on the next sync, whenever a plugin stops applying rather than only when Symposium itself is removed.

What Symposium does not delegate is equally definite. Predicates and `depends-on` are evaluated before a directory is produced, so an agent never receives a gate and never resolves one. Hooks remain registered and dispatched by Symposium, which is what allows a hook to be authored once in a vendor-neutral format and evaluated per dispatch. Installations, custom predicates, and subcommands have no portable representation and stay where they are.

### Version one

Version one delivers:

- a compiled plugin directory produced from an already-gated Symposium plugin, carrying its manifest and its resolved skills;
- installation of that directory for the agents that have a plugin unit (Claude Code, Codex CLI, the Copilot CLI, Antigravity CLI and Goose), carrying every manifest dialect at once: at user scope for all of them, and at project scope for Claude Code;
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
  .symposium                      ownership marker
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

$ cargo agents use pdf-tools
enabled pdf-tools for /home/alex/work/reporter
installed pdf-tools
  claude        plugin       .claude/skills/pdf-tools
  codex         skills only  .agents/skills/extract-tables
  antigravity   skills only  .agents/skills/extract-tables
```

`use` records the enablement, workspace-scoped unless `--global` is given, and then syncs. The compiled directory is written once under `.symposium/plugins/` for a workspace-scoped plugin, or under `~/.symposium/installed/` for a global one. Each agent is then given it by whichever mechanism it supports, which is why the lines differ: this enablement is workspace-scoped, and in version one only Claude Code receives a project-scoped plugin as a plugin, through a copy of the directory in the project's `.claude/skills/`, while Codex CLI and Antigravity CLI fall back to per-project skill directories. Had the same plugin been enabled with `--global`, all three would have received it as a plugin, installed once for the user.

```console
$ cargo agents status
active
  pdf-tools    agent plugin    used in this workspace
```

The agent is then started as usual and the skill is available. Whether it truly arrived is confirmed by asking the agent what it can see, since a file in a plausible location can still be ignored.

```console
$ cargo agents use pdf-tools --remove
disabled pdf-tools for /home/alex/work/reporter
removed pdf-tools from claude, codex, antigravity
```

Two other routes reach the same place without naming a plugin. Adding a dependency whose source carries plugin content produces a consent question on the next sync, and answering yes installs it exactly as above. Placing a `plugin.json` beside a `skills/` directory in a workspace member requires no command at all, because workspace membership is itself the gate.

### Compiled plugin model

The compiled model is a mapping rather than a rename, because a Symposium plugin holds more than the shared format can express. Skill groups from any source, including `source.git` groups that must be fetched first, are resolved into a self-contained `skills/` directory. The manifest carries the plugin's name, normalized to lowercase letters, digits and hyphens so that every agent accepts it; its version, `0.0.0` unless the plugin declares one; and its description when it has one. Everything else either stays with Symposium or has already been consumed.

No agent needs the version to change before it picks up an edit: Claude Code, the Copilot CLI, Antigravity CLI and Goose read their directory in place, and Symposium rewrites Codex CLI's cached copy itself.

Gating is the significant part. Because predicates are evaluated before compilation, the emitted directory contains only what applies to the current workspace. Nothing is lost by the format's lack of an activation vocabulary, because the format never needs one.

### Installation

Symposium writes each compiled directory once, into a location it owns, and prefers to tell an agent where that directory is rather than copy it into the agent's own plugin folder, since registering a path keeps one copy per plugin instead of one copy per plugin per agent. It registers a path only where the agent then reads the directory in place, and copies the directory everywhere else.

Two mechanisms therefore exist, and which one applies is a property of the agent. The Copilot CLI is the one agent registered by path: its settings name the staging root as a directory marketplace and enable the plugin from it, and Copilot reads the plugin where Symposium wrote it. Every other agent with a plugin unit gets a copy. Claude Code gets one in its user skills directory, where it loads as a skills-directory plugin with no configuration written; Antigravity CLI and Goose each get one in the folder they discover plugins in; and Codex CLI gets one in its own plugin cache, next to a marketplace and an enablement entry in its `config.toml`, because a Codex session loads a plugin only from that cache and Codex refreshes the cache on its own only when the version changes. [Agent backends](#agent-backends) records which agent falls where, and what that was verified against.

Claude Code also accepts a local directory marketplace, which would let it read the staging root in place. It was not chosen: Claude Code then keeps marketplace and installation records of its own, which a removal would have to edit to leave nothing behind, and at project scope two repositories using the same marketplace name loaded each other's content.

In both cases Symposium writes the agent's files itself rather than driving the agent's own plugin install command. This is what it already does for hook registration and MCP entries, and it is the only option on a path that runs during a hook, where there is no terminal and no user to answer a prompt. Because a well-formed file in a plausible location can still be ignored, the check that an installation worked is to ask the agent what it can see, not to inspect the file that was written.

Nothing about how a plugin is chosen changes. `cargo agents sync` compiles and installs whatever currently applies, `use` and `use --remove` record an enablement decision and then sync, and auto-sync performs the same work at session start. Installation remains a consequence of what applies to the workspace rather than a separate action, so this introduces no new command and no per-plugin install step.

Auto-sync runs in the session-start hook, and every plugin agent that runs that hook has built its list of plugins before the sync installs anything. A plugin installed there therefore appears in the next session, or sooner where the agent can reload: after `/reload-plugins` in Claude Code, `/restart` in the Copilot CLI, or `/skills reload` in Antigravity CLI. Goose runs no Symposium hook, but it looks for plugins on every message, so it picks up a plugin a sync installs from its next message.

### Scope of an installation

An installation is scoped to the enablement that produced it. A plugin named by a `use --global` entry installs for the user, whatever its gate and whatever `hook-scope` says; `hook-scope` keeps deciding only where Symposium registers its own hook and MCP servers. Everything else that applies installs for the project: a workspace-scoped `use` entry, a registry plugin active by its own gate, a workspace member, and a consented dependency. This is the scoping `use` already has, applied to a different artifact. An agent that receives a global plugin stops receiving per-project copies of that plugin's skills.

Predicates are still evaluated in the workspace being synced, while a user-level installation is visible from every workspace. What a global plugin carries therefore reflects the workspace synced last: a skill gated on a dependency, or a crate resolved at the workspace's version, can change when another workspace syncs, and a plugin that does not apply there leaves user scope until a workspace where it applies syncs. This is a known limitation rather than something tested for. `--global` is the user's statement that the plugin belongs everywhere, so it is installed everywhere. An earlier draft installed globally only a plugin whose gate cannot vary by workspace ([A global installation needs a workspace-independent gate](#a-global-installation-needs-a-workspace-independent-gate)); that test is not applied.

A project-scoped plugin reaches Claude Code as a plugin: a copy of the compiled directory in the project's `.claude/skills/`, which Claude Code loads as a skills-directory plugin once the folder is trusted. Codex CLI, the Copilot CLI, Antigravity CLI and Goose can each scope a plugin to a project too, but each way carries costs that version one does not take on ([Future work](#future-work)), so on those agents, as on OpenCode and Kiro, a project-scoped plugin keeps arriving through the per-skill path. Either way a workspace-scoped plugin never becomes visible in unrelated projects.

### Directory collisions

Several packages may sit side by side under one parent. Each is its own entry, which is what a registry already is, and no relationship between siblings is implied.

A `plugin.json` inside a directory that has already been claimed is ignored, because a claimed directory is not descended into. Nesting a package inside a package is therefore not a way to ship two, and the outer manifest is the one that describes the entry. A source root that is itself a package is an error, as it already is for the other two manifests: a source contains packages rather than being one.

On the install side, two plugins from different origins can resolve to the same directory name. The rule that already governs skill directories governs these: the plain name is used when the slot is free, and a name suffixed with a short hash of the package's origin is used when more than one origin claims the name or when a user-managed directory already occupies it. Cleanup keys on the ownership marker rather than on the shape of the name, so a package that moves between the two forms self-heals on the next sync.

### Agent backends

There is one backend, not one per agent. It renders the compiled model into the manifests described in [Change in a nutshell](#change-in-a-nutshell) and installs the result by whichever mechanism the agent supports: pointing the agent at the directory where it was written, or copying it into a folder the agent reads. Claude Code's manifest is a renderer over that one model rather than a separate design. Each agent's delivery is reconciled against the set of compiled plugins on every sync, so a plugin missing from the set is removed from the agent, whether it stopped applying or moved to project scope, and an agent removed from the configuration is reconciled against an empty set, which removes everything Symposium installed for it.

Which agent gets which mechanism, at which path, and what each of those was verified against belongs with the user-facing documentation; see [How extensions are installed](./proposed-install.md#how-each-agent-is-given-the-directory). Those mechanisms were verified in October 2026 against Claude Code 2.1.284, Codex CLI 0.159.3, the Copilot CLI 1.0.89, Antigravity CLI 1.2.14 and Goose 1.53.0, by asking each agent what it loaded. Where they differ from the August probes recorded in [Implementation details](#implementation-details), the October results are the ones this proposal follows.

OpenCode extends through TypeScript modules and offers no directory-shaped plugin unit, so it retains the per-skill install path. So does Kiro: its powers are Agent Plugins packages with more required fields, installed through Kiro's own interface, and no running Kiro was available to verify a location it would discover them in. Goose does offer a plugin unit, since 1.40: a root `plugin.json` beside a `skills/` folder, discovered by its presence in a folder Goose reads.

The per-skill path and plugin delivery meet in `.agents/skills/`, which Antigravity CLI, Codex CLI, the Copilot CLI, Goose and OpenCode all read. A global plugin's skills stay copied there while any configured agent that reads that directory does not take the plugin, which is always the case with OpenCode. An agent that did take the plugin then sees each of those skills twice: Codex CLI and Antigravity CLI list both, while the Copilot CLI and Goose use the copy in place of the plugin's skill. The content is the same either way; this is a known limitation of version one.

### External packages

Symposium recognizes two kinds of plugin directory today, one holding a `Symposium.toml` and one holding a bare `SKILL.md`. A directory holding a `plugin.json` becomes a third. Precedence runs `Symposium.toml`, then `plugin.json`, then `SKILL.md`. A directory carrying both a TOML and a JSON manifest loads as a Symposium plugin and takes its name, version and description from `plugin.json` where the TOML omits them; `version` and `description` are new optional fields of the Symposium manifest, so a plugin can declare them without a `plugin.json`. A directory holding the `.symposium` marker file is Symposium's own output, such as a compiled directory, and is never read back as a source.

Such a package is recognized in the three positions a plugin already occupies, and each position retains its existing meaning. A registry entry is curated and trusted. A workspace member is gated by membership. A dependency is an untrusted offer subject to consent.

Skills map without adaptation: `skills/` holds one skill per immediate child, in the `agentskills.io` format Symposium already uses, and deeper directories are not searched.

### Activation of external packages

The format cannot express when a package applies, so an externally authored package arrives without a gate. The existing enablement rules already resolve this. A plugin with nothing from which to infer a gate is dormant and is woken by a `use` entry; membership gates a workspace plugin; consent gates a dependency-embedded one. No new activation state is introduced, and `use`, `search`, and `status` describe these packages in the vocabulary they already use.

An author who wants a portable package to be dependency-scoped under Symposium declares `depends-on` or `predicates` beneath a `dev.symposium` key in the manifest's `extensions` object. The format requires a client to ignore namespaces it does not implement, and to do so without inspecting their contents, so the package remains portable.

### Failure containment

The format requires failures to be contained to the smallest affected unit and reported rather than suppressed, which matches how the report layer behaves. A manifest that violates its schema rejects that package alone and leaves the rest of the registry loading. An unrecognized top-level manifest field is reported and ignored. An invalid skill is skipped.

Containment is a security point: a package supplies only what is inside its own folder, so a path that resolves outside it is denied at the narrowest applicable boundary. A manifest that resolves outside rejects the package, a `skills` folder that does invalidates the skills component, and a `SKILL.md` that does skips that skill. Inside a skill, a linked file or folder that resolves outside the package is skipped and reported while the rest of the skill installs, on every path that copies a skill. This is narrower than the rule for other skill sources, whose links are followed wherever they point because the boundary there is enablement ([Symlinks in skill content](../../reference/skill-definition.md#symlinks-in-skill-content)); a portable package is held to the boundary its format defines.

### Ordering

Compilation comes first because it changes how everything installs and is therefore the baseline the rest builds on. Global delivery goes with it rather than after it: user scope is the one scope every agent with a plugin unit takes as a plugin in version one, while project scope reaches Claude Code alone, so a compile step that handled workspace scope alone would leave the per-skill path carrying almost everything. Accepting external packages adds a source of plugins. It does not depend on compilation and can land beside it, but it is considerably less valuable while those plugins would still be installed by the older per-skill mechanism.

## Frequently asked questions

### Which command installs a plugin?

None that is new. `cargo agents sync` compiles and installs whatever applies, `use` and `use --remove` record an enablement decision and then sync, and auto-sync does the same work at session start. A plugin is installed because it applies, not because it was installed by name.

### Why does `--global` install for the user even when the gate depends on the workspace?

Because `--global` is an explicit request to have the plugin everywhere. The test an earlier draft proposed would have turned that request into a per-project installation for nearly every gated plugin, and widening it would have needed a rule for every predicate kind. Taking the request as given costs one known limitation, described in [Scope of an installation](#scope-of-an-installation): what the plugin carries reflects the workspace synced last.

### Why not drive each agent's own plugin install command?

Symposium already writes agent configuration directly for hooks and MCP entries, and using one mechanism keeps that consistent. An agent's install command also assumes a marketplace and a user at a terminal, neither of which is available on the auto-sync path, which runs inside a hook.

### Why register a path instead of copying into each agent's folder?

Copying reproduces the problem this change is meant to remove, one copy per agent, one level higher, so a path is registered wherever the agent then reads the directory in place. In practice that is the exception: only the Copilot CLI is registered by path. Claude Code accepts a directory marketplace, but at the cost described in [Installation](#installation). Codex CLI accepts a path but loads a plugin only from its own cache, so the copy is needed anyway. Antigravity CLI and Goose discover plugins by their presence in a folder, and a path registered with Antigravity CLI loads the plugin's skills without the plugin's name in front of them. So copying is what most agents get.

### Why compile a directory rather than continue writing skill files directly?

Because the agents already implement a loader for that directory, and using it removes work instead of adding it. Symposium currently maintains a skills path per agent and per scope. Afterwards it maintains one fact per agent: how that agent is given a plugin directory.

### Is compilation lossy?

No. Predicates are evaluated before compilation, so an agent receives only what applies. Hooks, installations, custom predicates, and subcommands remain with Symposium and are dispatched by Symposium. What is handed over is the skills, which is precisely what is scattered today.

### Does Claude Code need a separate emitter?

No. Its plugin format predates the standard and differs mainly in the manifest path and in supporting components the standard omits. Antigravity CLI's dialect of the root `plugin.json` is a similar case: it reads the fields it knows and drops the rest. Because each agent ignores the manifests that are not its own, one compiled directory can carry both manifests at once, so these are manifest renderers over one model rather than emitters of their own. Every agent with a plugin unit loaded the same compiled directory; see [How each agent is given the directory](./proposed-install.md#how-each-agent-is-given-the-directory).

### What happens to OpenCode and Goose?

OpenCode has no plugin unit to target, so it continues to receive skills as individual directories, and so does Kiro until its powers are verified. Goose has one: a plugin enabled with `--global` reaches it as a plugin directory copied into `~/.agents/plugins/`, the way Antigravity CLI receives one. A project-scoped plugin reaches Goose as individual skill directories in version one, as it reaches every agent except Claude Code.

### Why `dev.symposium` for the extensions namespace?

The format asks a client to base its namespace on a domain it controls and to keep that namespace stable, since published identifiers should not move. `dev.symposium` corresponds to the organization the project publishes from, and control of the matching domain is confirmed before the identifier is published.

### Is the format stable enough to build on?

Version 1.0.0 was published in August 2026 by a committee spanning Amazon, Cursor, Microsoft, OpenAI, Vercel, and Google. Its schemas are versioned with the specification text, and published schema identifiers cannot be reassigned to different contents. The surface is two component types and a closed manifest, which bounds the cost of being wrong.

## Implementation plan

Each step names the pull requests that deliver it. A step that installs for an agent is verified by asking the running agent what it loaded after a sync (its listing commands, its slash commands, and the request a session sends its model), not by inspecting the files written to it.

### Step 1: Verify each agent's plugin mechanics

Confirm each agent's plugin location, manifest dialect, enablement mechanism, and the scoping it can express. Probed in August 2026, as recorded in [Implementation details](#implementation-details), and again in October 2026 at user and project scope against Claude Code 2.1.284, Codex CLI 0.159.3, the Copilot CLI 1.0.89, Antigravity CLI 1.2.14 and Goose 1.53.0; the October results are what this proposal describes. Kiro was not available and remains to be probed.

### Step 2: Compile global plugins

[#332](https://github.com/symposium-dev/symposium/pull/332) compiles a plugin named by a `use --global` entry into `~/.symposium/installed/<name>/`, carrying both manifests with the 1.0.0 `$schema`, the ownership marker and its skill directories, under a name every agent accepts and disambiguated on collision. The staging root carries the `.claude-plugin/marketplace.json` index that the marketplace-based agents read. Tests: tables for name normalization and for what a compiled directory holds, snapshots of the manifests and the index, and integration tests over a fixture holding a global, a workspace-scoped and an always-active plugin.

### Step 3: Install global plugins for each agent with a plugin unit

One function per agent reconciles its delivery with the compiled set, and an agent that takes a plugin stops receiving per-project copies of its skills:

- Claude Code, [#332](https://github.com/symposium-dev/symposium/pull/332): a copy in its user skills directory. [#333](https://github.com/symposium-dev/symposium/pull/333) makes Symposium's Claude Code hooks and MCP servers follow `CLAUDE_CONFIG_DIR`, as the plugin does.
- Codex CLI, [#334](https://github.com/symposium-dev/symposium/pull/334): the marketplace and enablement entries in `config.toml` and a copy in its plugin cache. A separate fix makes Symposium's Codex hooks and MCP server follow `CODEX_HOME`, as the plugin does.
- Copilot CLI, [#346](https://github.com/symposium-dev/symposium/pull/346): the marketplace and enablement entries in `settings.json`, read in place. [#345](https://github.com/symposium-dev/symposium/pull/345) makes Symposium's Copilot hooks and MCP servers follow `COPILOT_HOME`, as the plugin does.
- Antigravity CLI and Goose: a copy in the folder each discovers plugins in. A separate fix makes Symposium's Goose MCP registration follow `GOOSE_PATH_ROOT`, as the plugin does.

Tests: per agent, a table over what Symposium writes for it (created, rewritten only on change, the user's own entries and a user's disable kept, stale entries removed, a foreign entry left alone with a warning, malformed configuration left untouched), and integration tests for install, edit, removal and agent removal. Each agent is then asked what it loaded after a sync, after a second sync, after a same-version edit, after removal, with its relocation variable set, after an install during the session-start hook, and next to another configured agent.

### Step 4: Install project-scoped plugins for Claude Code

Plugins that apply to the workspace and are not global are compiled into `<project>/.symposium/plugins/`, kept out of version control by a `.gitignore` at the root of `.symposium/`, and copied into the project's `.claude/skills/<name>/`, retiring Claude Code's per-skill copies there. Tests: integration tests showing a plugin delivered in one workspace and absent from another, and Claude Code asked what it loaded in two repositories with the same plugin name, trusted and untrusted.

### Step 5: Recognize `plugin.json` packages

A `plugin.json` directory becomes a plugin entry in every position a plugin is already found, including the nesting and source-root rules, and a directory carrying the ownership marker is never read back as a source. Tests: a table over every combination of the three manifest files, and enumeration tests for nesting and for the marker.

### Step 6: Resolve an external package's skills and contain failures

A package's skills resolve, one per immediate child of `skills/`; an `mcp.json` is reported as unsupported; malformed packages and skills are contained at their boundaries; and the manifest, the `skills` folder and each `SKILL.md` must resolve inside the package. A separate change applies the same boundary to links inside skill content, on every path that copies a skill. Tests: a translator table over the format's rules, and a registry package whose skill links a file outside the package, next to the same layout under a `Symposium.toml` plugin, which keeps following links.

### Step 7: Honor `dev.symposium` and cover the commands

The `dev.symposium` namespace gates a package, `version` and `description` become optional manifest fields, and `plugin validate`, `search`, `status` and `use` cover these packages. Steps 5 to 7 land together. Tests: integration fixtures for a registry, a workspace-member and a dependency package, and snapshots of `plugin validate` output.

### Future work

Emitting and reading `mcp.json` removes the last per-agent configuration format Symposium maintains, and follows the meta-server decision described above. It also introduces the format's subprocess contract, a `PLUGIN_ROOT` and a `PLUGIN_DATA` directory that survives updates, and requires a rule separating Symposium's own `${VAR}` expansion from the format's two placeholders.

Project scope for the other agents with a plugin unit is left for later. Each can scope a plugin to a project, verified in October 2026, but each way has a cost that version one does not take on, so their project-scoped plugins keep the per-skill path until those costs are settled:

| Agent | Project mechanism | Cost |
|---|---|---|
| Codex CLI | a marketplace and an enablement entry in the project's `.codex/config.toml`, plus a copy in the user-level plugin cache | honored only in a trusted project and silently ignored otherwise; the cache is keyed by marketplace name, so each project needs a name of its own, and nothing removes a project's copy once the project is gone; the entry needs an absolute, machine-specific path in a file teams may commit |
| Copilot CLI | the same two entries in the repository's `.github/copilot/settings.local.json` | read only at the git root, so a workspace nested in a repository has to write outside itself; the file has to be kept out of version control; the marketplace name must differ from the user-level one, which wins a clash |
| Antigravity CLI | a copy in `<project>/.agents/plugins/<name>/` | the folder is shared with Goose, so one writer has to serve both; an interactive session loads it only in a trusted folder, while `agy -p` loads it in any folder |
| Goose | a copy in `<project>/.agents/plugins/<name>/` | the same shared folder; read only from the working directory, not from a subdirectory of the project |

Kiro packages its extensions as powers, which are Agent Plugins packages with more required fields. Verifying against a running Kiro where installed powers live, and whether it discovers one placed there without its own install command, decides whether it joins the agents with a plugin unit.

Beyond that, Claude Code and Goose can both carry hooks inside a plugin directory, which would trade the vendor-neutral hook format and per-dispatch predicates for native dispatch. Publishing a compiled Symposium plugin as a portable package for other clients is a short step once compilation exists. A package's `version` can drive update checks and cache freshness, which the format explicitly permits.

## Implementation details

What probing the installed agents and mapping this proposal onto the current code turned up. The first two points constrain the implementation; the rest corrected claims in the sections above, which now read as amended.

### Where a global compiled directory goes

[Installation](#installation) writes a global plugin's directory under the user configuration directory. That directory already holds one: `~/.symposium/plugins/` is the builtin `user-plugins` registry (`Symposium::registry_instances` in `src/config.rs`), a directory symposium *reads* entries from.

Compiling into it makes symposium read its own output. A compiled directory carries a `plugin.json`, which registry enumeration claims as an entry once [external packages](#external-packages) are recognized, so every global install reappears as a plugin of the `user-plugins` registry on the next load.

Global compiled directories go in a sibling instead:

```text
~/.symposium/plugins/     registry symposium reads
~/.symposium/installed/   compiled directories symposium writes
```

Workspace scope is unaffected (`<project root>/.symposium/plugins/`, as described above), and the whole `.symposium/` directory is symposium's own, so one `.gitignore` containing `*` sits at its root rather than one per compiled directory.

### A global installation needs a workspace-independent gate

[Scope of an installation](#scope-of-an-installation) matches an installation to the enablement that produced it, so a `use --global` entry installs for the user. Predicates, though, are evaluated against the workspace being synced, while a user-level directory is visible from every workspace. The two do not compose:

```console
$ cargo agents use --global pdf-tools   # gated depends-on = ["lopdf"]
```

Run where `lopdf` is a dependency, the gate holds and the directory is written under the user configuration directory. Every other project on the machine now sees the plugin, without the dependency that gates it.

Removal has the mirror problem. Cleanup removes marked directories it did not install on this run, so a global set that varies by workspace makes two projects undo each other: syncing the project without `lopdf` removes the directory, syncing the one with it writes it back, on every session start.

Both follow from evaluating a workspace-dependent gate once and keeping the result somewhere workspace-independent. A plugin therefore installs globally only when its gate cannot vary by workspace: an empty gate (a dormant plugin woken by a global `use` entry, which is the case [the walkthrough](#walkthrough) shows) or `depends-on = ["*"]`. A global entry on any other plugin installs per project, and the install report names the scope it got.

The test is conservative. `depends-on(<name>)` and `workspace-member()` are workspace-dependent by definition, `shell(...)` and a relative `path_exists(...)` resolve against the workspace as their working directory, and a custom predicate is opaque. Only the empty set and `depends-on(*)` qualify; widening the test per predicate kind later is additive.

### One directory, several manifests

This is the evidence for the single directory [Change in a nutshell](#change-in-a-nutshell) shows. An earlier draft treated Claude Code and Gemini CLI as separate emitters; probing the installed agents shows they do not need to be. Claude Code ignores a `plugin.json` at a package root and falls back to the directory name for identity, Agent Plugins clients ignore `.claude-plugin/`, and Gemini CLI reads only its own file. So one compiled directory carrying all three manifests, with the same content in each, satisfies every one of them:

```text
pdf-tools/
  plugin.json                    Codex CLI, Copilot CLI, VS Code
  .claude-plugin/plugin.json     Claude Code
  gemini-extension.json          Gemini CLI
  skills/extract-tables/SKILL.md every one of them
```

Verified against Claude Code 2.1.237, Codex CLI 0.147.0, Copilot CLI 1.0.79 and Gemini CLI 0.55.1: both validators accept that directory, and Claude Code resolves the declared name from it rather than the directory name.

The staging root needs one manifest of its own, because Claude Code, Codex CLI and Copilot CLI all take a *marketplace* (a directory of packages) plus a per-package enablement entry, rather than a path to one package. `.claude-plugin/marketplace.json` is the one file all three read. Codex CLI also accepts `.agents/plugins/marketplace.json` and prefers it when both are present; Claude Code rejects it.

So there is one emitter with several manifest renderers, not one emitter per agent.

### Only Claude Code can scope a plugin to a project

This is the evidence for [Scope of an installation](#scope-of-an-installation). An earlier draft expected Codex CLI, Kiro and Claude Code to offer a project-scoped location, and VS Code to accept a workspace-scoped path registration. Of the agents installed here, only Claude Code does:

| Agent | Result |
|---|---|
| Claude Code | `plugin install -s project` |
| Codex CLI | a project `.codex/config.toml` marketplace entry is ignored, and there is no folder discovery at `.codex/plugins/` or `$CODEX_HOME/plugins/` |
| Copilot CLI | a project `.copilot/settings.json` is ignored, a working-directory marketplace manifest is not discovered, and `plugin install` has no scope flag |
| Gemini CLI | a project `.gemini/extensions/` is ignored |
| VS Code | `chat.pluginLocations` is machine-scoped and workspace-trust restricted, so it cannot be set per workspace |

Kiro is not installed here and remains unconfirmed.

The consequence is that a workspace-scoped plugin can be delivered as a compiled directory to Claude Code alone. On the others it arrives as individual skill directories, as it does today. So the per-skill path is not reduced in scope: it stays the mechanism for workspace-scoped plugins on six of the seven agents, and compilation pays off for global installations plus Claude Code project installations. That is why [Ordering](#ordering) puts global delivery in the first increment rather than in follow-up work.

### Two mechanics worth recording

Codex CLI's install step copies a package into a version-keyed cache under `$CODEX_HOME/plugins/cache/`. Writing its `config.toml` entries alone registers enablement against an empty cache, so the copy has to happen too. Re-running the install after editing the source at the same version does re-copy, so there is no stale-content trap. Its removal step clears both config tables but leaves the cache directory behind, which symposium has to delete itself.

Gemini CLI needs no configuration write at all: a package present at `~/.gemini/extensions/<name>/` is listed and enabled. Its own `extensions link` command blocks waiting on input, which is another reason the file is written directly rather than shelling out.

## Implementation status

This RFD describes proposed work. Implementation has not begun.

See [Proposed: Agent Plugins packages](./proposed-reference.md) for the intended authoring reference and [Proposed: How extensions are installed](./proposed-install.md) for the resulting install locations.
