# How extensions are installed

> Proposed page for the [Agent Plugins interoperability](./README.md) RFD. This is how the install-locations part of [Agents](../../design/agents.md) would read once the compile step lands.

Symposium decides which plugins apply to your workspace, then hands each agent the result. For the agents that have a plugin unit it hands over a **plugin directory**, which is the unit those agents use themselves.

## Which command installs a plugin

There isn't one. A plugin is installed because it applies, not because you install it by name.

```bash
cargo agents sync                  # installs whatever currently applies
cargo agents use <name>            # enables a plugin, then syncs
cargo agents use <name> --remove   # disables it, then syncs
cargo agents status                # shows each plugin, why it applies, and where it went
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
  claude        plugin       .claude/skills/pdf-tools
  codex         skills only  .agents/skills/extract-tables
  antigravity   skills only  .agents/skills/extract-tables
```

The lines differ because agents accept a plugin differently, and because this enablement is scoped to one project. For a single project, only Claude Code gets the plugin itself, as a copy of the directory in the project's `.claude/skills/`; Codex CLI and Antigravity CLI get this project's skills the older way instead, as individual skill directories. Run `use --global` and all three get the plugin, installed once for you in every project.

Start your agent as usual and the skill is there. To be sure, ask the agent what skills it can see.

Undo it the same way:

```console
$ cargo agents use pdf-tools --remove
disabled pdf-tools for /home/alex/work/reporter
removed pdf-tools from claude, codex, antigravity
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

For each plugin that applies, symposium compiles a directory, under `~/.symposium/installed/` for a plugin you enabled with `--global` and under `.symposium/plugins/` in your project otherwise:

```text
pdf-tools/
  plugin.json                  who this plugin is
  .claude-plugin/plugin.json   the same, for Claude Code
  .symposium                   marks the directory as ours, so we can clean it up later
  skills/
    extract-tables/
      SKILL.md
```

One directory serves every agent. Each one reads the manifest it knows and ignores the others.

The manifest carries the plugin's name, in lowercase letters, digits and hyphens so that every agent accepts it, and its version, which is `0.0.0` unless the plugin declares one. In your project, `.symposium/` holds a `.gitignore` that keeps everything under it out of your commits.

Only what applies is in there. Predicates and `depends-on` are evaluated before this directory is built, so the agent never sees a gate and never loads something it should not have.

Skills from a `source.git` group are fetched and resolved first, so the directory is self-contained.

## How each agent is given the directory

The directory is written once. An agent that reads a registered directory in place is pointed at it; every other agent gets a copy in a folder it reads. For a plugin you enabled with `--global`:

| Agent | Manifest it reads | How it is given | Verified against |
|-------|-------------------|-----------------|------------------|
| Claude Code | `.claude-plugin/plugin.json` | Copied into `~/.claude/skills/<name>/` (under `$CLAUDE_CONFIG_DIR` when set), where it loads as a skills-directory plugin; no settings are written | 2.1.284 |
| Codex CLI | `plugin.json` | The `symposium` marketplace and an enablement entry in `~/.codex/config.toml`, plus a copy in `~/.codex/plugins/cache/symposium/<name>/<version>/`, the only place a Codex session loads a plugin from (both under `$CODEX_HOME` when set) | 0.159.3 |
| GitHub Copilot CLI | `plugin.json` | The `symposium` marketplace, pointing at `~/.symposium/installed/`, and an enablement entry in `~/.copilot/settings.json` (under `$COPILOT_HOME` when set); Copilot reads the directory where it is. Needs 1.0.81 or later | 1.0.89 |
| Antigravity CLI | `plugin.json`, in its own dialect | Copied into `~/.gemini/config/plugins/<name>/` | 1.2.14 |
| Goose | `plugin.json` (Open Plugins) | Copied into `~/.agents/plugins/<name>/` (under `$GOOSE_PATH_ROOT` when set) | 1.53.0 |
| Kiro | | Not yet confirmed; gets individual skill directories, as before | |

Claude Code, Codex CLI and Antigravity CLI show a plugin's skills under the plugin's name, as in `pdf-tools:extract-tables`. The Copilot CLI and Goose show them under the skill's own name.

Symposium writes these files itself, the same entries and copies the agent's own install command produces, rather than running that command. It registers hooks the same way, and on the auto-sync path there is no terminal to answer a prompt.

Symposium touches only what is its own: the `symposium` marketplace, the `<name>@symposium` enablement entries, and directories carrying its marker. If you turn one of its plugins off in the agent, that choice is kept across syncs. To remove a plugin for good, use `cargo agents use <name> --remove --global` rather than the agent's own uninstall command, which the next sync may undo. A marketplace called `symposium` that points somewhere else, or a directory with the plugin's name that symposium did not write, is never overwritten; symposium warns instead.

## When a new plugin shows up

An agent reads its plugins when a session starts. A plugin installed by `cargo agents sync` is there in the next session you start.

Auto-sync runs while a session starts, after the agent has read its plugins, so a plugin it installs is there in your next session. Some agents can pick it up sooner: `/reload-plugins` in Claude Code, `/restart` in the Copilot CLI, `/skills reload` in Antigravity CLI. Goose looks for plugins on every message, so it picks up a newly installed plugin from your next message.

## Project or global

An installation matches the scope of whatever enabled the plugin. A `use --global` entry installs it for you, in every project. Anything else that applies (a workspace `use` entry, a workspace member, one of your dependencies, a registry plugin whose `depends-on` matches this project) installs for this project only.

For a project, Claude Code gets the plugin itself, copied into the project's `.claude/skills/<name>/`, where it loads once you trust the folder. The other agents get a project's plugins the older way, as individual skill directories under the project. Either way, a plugin enabled in one project never shows up in another.

A global installation has one known limitation. Predicates and `depends-on` are evaluated in the project being synced, but the installation is visible from every project. So what a global plugin carries, such as a skill that applies only with a certain dependency, reflects the project you synced last, and a plugin that applies nowhere in that project is removed until you sync a project where it does.

## Individual skill directories

OpenCode has no plugin directory to install into: it extends through TypeScript modules. Kiro has one, its powers, but how Kiro installs them has not been verified yet. For these two, symposium installs skills the old way, as individual skill directories: `.agents/skills/<skill-name>/` for OpenCode and `.kiro/skills/<skill-name>/` for Kiro. Every agent except Claude Code gets a project-scoped plugin the same way.

`.agents/skills/` is read by Antigravity CLI, Codex CLI, the Copilot CLI, Goose and OpenCode alike. While one of them still needs a global plugin's skills copied there, as OpenCode always does, the agents that also have the plugin see those skills twice: Codex CLI and Antigravity CLI list both, while the Copilot CLI and Goose use the copy instead of the plugin's skill. The content is the same either way.

## When two plugins want the same name

The plain plugin name is used when it is free. When two plugins from different sources want the same name, or when a directory you manage yourself already sits there, symposium adds a short hash of the plugin's origin to the directory name instead.

Cleanup keys on the `.symposium` marker rather than the directory name, so a plugin that moves between the two forms sorts itself out on the next sync.

## What is not in the directory

**MCP servers.** Registered separately. See [plugin definitions](../../reference/plugin-definition.md) for how registration works and what `[mcp] enabled` controls.

**Hooks.** Symposium registers only its own hook handler with an agent, and dispatches plugin hooks itself. This is what lets a hook be written once in a vendor-neutral format and still run on every agent that supports hooks, and it is why a hook's predicates are checked on each dispatch rather than once at install time.

## What happens to old files

Every directory symposium installs carries a `.symposium` marker. On each sync it removes marked directories it no longer owns, and leaves anything unmarked alone, so your own skills and plugins are never touched. The entries it wrote into an agent's configuration are removed the same way.

Directories written by a configuration you have switched away from are removed in the same pass. Moving between the plugin-directory install and the older per-skill install does not leave anything behind, and neither does removing an agent from your configuration.
