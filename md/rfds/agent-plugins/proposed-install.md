# How extensions are installed

> Proposed page for the [Agent Plugins interoperability](./README.md) RFD. This is how the install-locations part of [Agents](../../design/agents.md) would read once the compile step lands.

Symposium decides which plugins apply to your workspace, then hands each agent the result. For most agents it hands over a **plugin directory**, which is the unit agents themselves use.

## Which command installs a plugin

There isn't one. A plugin is installed because it applies, not because you install it by name.

```bash
cargo agents sync          # installs whatever currently applies
cargo agents use <name>    # enables a plugin, then syncs
cargo agents remove <name> # disables it, then syncs
cargo agents status        # shows each plugin, why it applies, and where it went
```

With auto-sync on, the same work happens when an agent session starts, so usually there is nothing to run at all.

## Installing a plugin, start to finish

Three routes, depending on where the plugin comes from.

### A plugin you ask for

Find it, then enable it. `use` scopes to the current workspace unless you pass `--global`, and syncs straight away.

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

The three lines differ because agents accept a plugin differently. Claude Code is pointed at the directory where it was written, Codex CLI gets a copy in its own folder, and Gemini CLI cannot scope a plugin to one project, so its skills are installed per project instead.

Start your agent as usual and the skill is there. To be sure, ask the agent what skills it can see.

Undo it the same way:

```console
$ cargo agents remove pdf-tools
disabled pdf-tools for /home/alex/work/reporter
removed pdf-tools from claude, codex, gemini
```

### A plugin that comes with a dependency

Add the dependency and sync. Symposium asks before installing anything a dependency carries, because a dependency is not something you chose to trust for this.

```console
$ cargo add lopdf
$ cargo agents sync
lopdf offers a plugin. Enable it?
  > enable
    ask me later
    never ask again
```

Answering `enable` installs it exactly as above and remembers the answer. `never ask again` records the refusal, and the default answer records nothing, so pressing Enter never decides anything permanently.

### A plugin in your own project

Put a `plugin.json` beside a `skills/` directory in your workspace, or in any member of it:

```text
reporter/
  Cargo.toml
  plugin.json
  skills/
    house-style/
      SKILL.md
```

Nothing to run and nothing to enable. Being part of the workspace you are working in is what makes it active, so it applies to everyone working in this project and to no one else.

## What symposium builds

For each plugin that applies, symposium compiles a directory:

```text
pdf-tools/
  plugin.json          who this plugin is
  .gitignore           contains *, so the directory stays out of your commits
  .symposium           marks the directory as ours, so we can clean it up later
  skills/
    extract-tables/
      SKILL.md
```

Only what applies is in there. Predicates and `depends-on` are evaluated before this directory is built, so the agent never sees a gate and never loads something it should not have.

Skills from a `source.git` group are fetched and resolved first, so the directory is self-contained.

## How each agent is given the directory

The directory is written once. Agents that can be pointed at a path are pointed at it; agents that only discover plugins by location get a copy.

| Agent | Format it gets | How it is given |
|-------|----------------|-----------------|
| VS Code / GitHub Copilot | Agent Plugins | Path registered through the plugin-locations setting |
| Claude Code | Claude Code plugin | Registered as a local marketplace, plus an enablement entry in settings |
| Codex CLI | Agent Plugins | Copied into `.codex/plugins/` for a project, `~/.codex/plugins/` for you |
| Kiro | Agent Plugins | Copied into `.kiro/plugins/` |
| Gemini CLI | Gemini extension | Copied into `~/.gemini/extensions/<name>/` |

Claude Code and Gemini CLI use their own manifest names and their own loaders, but the directory holds the same skills.

Symposium writes each agent's configuration itself rather than running that agent's own plugin install command, the same way it registers hooks.

## Project or global

An installation matches the scope of whatever enabled the plugin. A workspace `use` entry, a workspace member, or one of your dependencies installs for that project only. A `use --global` entry installs for you everywhere.

Some agents cannot express a project-scoped plugin: their plugin folder is per-user and they offer no way to register a path. For those, a project-scoped plugin is installed the older way instead, as individual skill directories under the project, so that a plugin enabled in one project never shows up in another.

## Agents without a plugin unit

OpenCode and Goose have no plugin directory to install into. OpenCode extends through TypeScript modules, and Goose through MCP servers. For these two, symposium installs skills the old way, into `.agents/skills/<skill-name>/`.

## When two plugins want the same name

The plain plugin name is used when it is free. When two plugins from different sources want the same name, or when a directory you manage yourself already sits there, symposium adds a short hash of the plugin's origin to the directory name instead.

Cleanup keys on the `.symposium` marker rather than the directory name, so a plugin that moves between the two forms sorts itself out on the next sync.

## What is not in the directory

**MCP servers.** Registered separately. See [plugin definitions](../../reference/plugin-definition.md) for how registration works and what `[mcp] enabled` controls.

**Hooks.** Symposium registers only its own hook handler with an agent, and dispatches plugin hooks itself. This is what lets a hook be written once in a vendor-neutral format and still run on every agent that supports hooks, and it is why a hook's predicates are checked on each dispatch rather than once at install time.

## What happens to old files

Every directory symposium installs carries a `.symposium` marker. On each sync it removes marked directories it no longer owns, and leaves anything unmarked alone, so your own skills and plugins are never touched.

Directories written by a configuration you have switched away from are removed in the same pass. Moving between the plugin-directory install and the older per-skill install does not leave anything behind.
