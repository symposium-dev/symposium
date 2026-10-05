# Authoring a plugin

Symposium lets you ship skills, hooks, and MCP servers to the AI agents of people whose projects depend on your crate. This page walks through how to create a plugin and configure each extension type.

## Step 1. Decide where your plugin lives

A plugin can live in one of two places:

- **In your crate.** If you maintain the crate, ship the plugin in its source tree: a `skills/` directory is enough, and a `SYMPOSIUM.toml` adds hooks, MCP servers, and more. See [Embedding skills in your crate](#embedding-skills-in-your-crate-recommended).
- **In the [central recommendations repository][rr].** This is the place for plugins for crates you don't maintain, and for plugins that should load without asking the user first.

A recommendations entry is a `SYMPOSIUM.toml` manifest declaring your plugin's name, which crates it applies to, and what extensions it provides:

```toml
# `my-crate/SYMPOSIUM.toml` on the symposium-dev/recommendations repository
name = "my-crate"
depends-on = ["my-crate"]
```

The `depends-on` field controls when the plugin is active: it only loads for projects that directly depend on the listed crates. Use `["*"]` to apply to all projects.

See the [plugin definition reference](../reference/plugin-definition.md) for the full manifest schema.

### Why do users enable plugins from dependencies?

Depending on a crate means compiling its code, not letting its author inject context into the user's agent. So a plugin embedded in a dependency stays off until the user enables it: an interactive [`cargo agents sync`](../reference/cargo-agents-sync.md#consent-prompt) asks about each one, and [`cargo agents use`](../reference/cargo-agents-use.md) enables one by name. Plugins in the [recommendations repository][rr] are reviewed before they are merged, so they load without that step, and a plugin that proves problematic can be removed centrally.

## Step 2. Add skills, hooks, and/or MCP servers

Wherever your plugin lives, you can add any combination of the extension types below.

### Skills

Skills are guidance documents that teach AI assistants how to use a crate. Each skill is a directory containing a `SKILL.md` file with YAML frontmatter and a markdown body:

```markdown
---
name: my-crate-basics
description: Basics of my-crate. Use when building or validating widgets with my-crate.
---

Prefer using `Widget::builder()` over constructing widgets directly.
Always call `.validate()` before passing widgets to the runtime.
```

See the [Skill definition reference](../reference/skill-definition.md) for the full format and the [agentskills.io quickstart](https://agentskills.io/skill-creation/quickstart) for writing effective skills.

#### Embedding skills in your crate (recommended)

If you maintain the crate, we recommend shipping skills directly in your source tree. This way users always get skills matching the exact version they have installed.

##### 1. Put skills in your crate sources under `skills/`

```
my-crate/
    Cargo.toml
    src/
        lib.rs
    skills/
        basics/
            SKILL.md
        advanced-patterns/
            SKILL.md
```

##### 2. Publish your crate

That is all Symposium needs. When a project depends directly on `my-crate`, an interactive `cargo agents sync` offers to enable its plugin, and [`cargo agents status`](../reference/cargo-agents-status.md) lists it as a candidate until the user decides. `cargo agents use my-crate` enables it directly, even for a project that doesn't depend on the crate.

Symposium reads your plugin from the published package, so make sure `skills/` (and `SYMPOSIUM.toml`, if you have one) are part of it: check with `cargo package --list` (add `--allow-dirty` if you have uncommitted changes), and add them to `include` if your `Cargo.toml` uses that field.

##### Optional: load without asking

To have the plugin load for every user who depends on your crate, without the consent step, add an entry to the recommendations repository that references your crate with a chained plugin:

```toml
# `my-crate/SYMPOSIUM.toml` on the symposium-dev/recommendations repository
name = "my-crate"
depends-on = ["my-crate"]

[[plugins]]
source.cargo = "my-crate"
```

When `my-crate` is a dependency, this loads its plugin: Symposium fetches the crate source (from the local cargo cache or crates.io) and discovers skills in the `skills/` directory.

##### Prefer a directory other than `skills/`?

Add `[package.metadata.symposium]` to your crate's `Cargo.toml` to declare your own skills directory. This block uses the same schema as a `SYMPOSIUM.toml` plugin manifest:

```toml
# In your crate's Cargo.toml
[[package.metadata.symposium.skills]]
source.path = "docs/agent-skills"
```

The default `skills/` group is kept next to yours (it finds nothing if the directory doesn't exist); set `[package.metadata.symposium.defaults] skills = false` to turn it off. With neither a metadata section nor a `SYMPOSIUM.toml`, Symposium uses only the `skills/` directory. See [Supporting your crate](./supporting-your-crate.md) for the full schema including chained references to other crates.

##### Ship a full `SYMPOSIUM.toml` in your crate

For more than a single custom directory (several skill groups, per-group predicates, a git skill source, hooks, or MCP servers), put a `SYMPOSIUM.toml` at your crate root. When the crate's plugin loads, whether the user enabled it or a `[[plugins]] source.cargo` reference reached it, that manifest is loaded as a first-class plugin:

```toml
# `my-crate/SYMPOSIUM.toml` (in your crate's source tree)
[[skills]]
source.path = "docs/agent-skills"

[[skills]]
depends-on = ["tokio"]
source.path = "docs/async-skills"
```

Because reaching your crate is already the gate, a crate manifest doesn't need `name` (it defaults to the crate) or a top-level `depends-on`. The default `skills/` group is still appended unless you opt out with `[defaults] skills = false`. The `[package.metadata.symposium]` block and a `SYMPOSIUM.toml` file are the same manifest schema and are combined when both are present: lists from both are kept, and where both set the same value the `SYMPOSIUM.toml` file wins. Use whichever is convenient.

Hooks, MCP servers, and subcommands declared in a crate `SYMPOSIUM.toml` load just like its skills.

#### Standalone skills (on the recommendations repo)

You can also upload skills directly to the [recommendations repo][rr] — without embedding them in the crate source. This is the right approach when you're writing skills for a crate you don't maintain.

Place skill directories alongside your `SYMPOSIUM.toml`:

```
my-crate/
    SYMPOSIUM.toml
    basics/
        SKILL.md
    advanced-patterns/
        SKILL.md
```

And point the manifest at the local directory:

```toml
name = "my-crate"
depends-on = ["my-crate"]

[[skills]]
source.path = "."
```

The manifest's `depends-on` says which crate the skills apply to, so the skills themselves don't need one.

A skill can also be an entry on its own: a directory in the recommendations repo holding just a `SKILL.md`, with no `SYMPOSIUM.toml`. Such a skill names the crates it applies to with `depends-on` in its frontmatter (or a `depends-on(...)` predicate in `predicates`). Without one it loads dormant, and only users who run `cargo agents use <skill-name>` get it:

```markdown
---
name: widgetlib-basics
description: Basics of widgetlib. Use when building or configuring widgets with widgetlib.
depends-on: widgetlib=1.0
---

Guidance body here.
```

#### Skills from a git repository

Symposium also supports fetching skills from a GitHub URL:

```toml
[[skills]]
source.git = "https://github.com/org/my-crate/tree/main/symposium/skills"
```

This is useful for hosting skills in a dedicated repository or a subdirectory of a monorepo. Note that the central recommendations repository does not currently accept `source.git` entries by policy — use a `[[plugins]] source.cargo` chained reference or `source.path` for submissions there.

### Installing auxiliary tools

An **installation** tells symposium how to obtain a binary that your hooks or subcommands will run. The recommended approach is a `cargo` installation, which installs a crate binary from crates.io:

```toml
[[installations]]
name = "my-crate-hooks"
source = "cargo"
crate = "my-crate-hooks"
executable = "my-crate-hooks"
```

Symposium installs the binary into its cache (`~/.symposium/cache/`) the first time a hook needs it, and checks [crates.io](https://crates.io/) for a newer release at the start of each agent session, unless the installation pins an exact `version`.

The binary must be installable with `cargo install`: publish it as its own crate, as above, or add a `[[bin]]` target to your main crate and set `crate = "my-crate"` with `executable` naming that binary.

See the [plugin definition reference](../reference/plugin-definition.md#installations) for other installation sources (GitHub repositories, local paths) and advanced options like `install_commands`.

### Hooks

Hooks run when the AI performs certain actions — invoking a tool, starting a session, or submitting a prompt. They receive JSON on stdin describing the event and can return guidance, inject context, or block the action.

Every agent varies in the specifics of what hooks it offers and how those hooks are configured. Symposium allows you to provide agent-specific hook handlers, but we recommend instead using a *Symposium hook* handler, which is portable across agents. Hooks run on every [supported agent](../reference/supported-agents.md) except Goose and OpenCode, which have no shell hooks Symposium can register (they still receive skills and MCP servers).

#### Symposium hooks (portable across agents)

To define a Symposium hook handler you add a `[[hooks]]` section. This defines the command to run as well as the events it expects and other filters. 

```toml
[[hooks]]
name = "check-usage"
event = "PreToolUse"
matcher = "^(Bash|bash|run_command|execute_bash)$"
command = "my-crate-hooks"
```

The `matcher` is a regular expression tested against the tool name. Agents name the same tool differently (the shell is `Bash` in Claude Code and Codex, `bash` in Copilot, `run_command` in Antigravity, and `execute_bash` in Kiro). The tool name reaches your handler exactly as the agent sent it, and the input keeps the agent's own argument names, so match every name you care about. See [Matcher](../reference/hook-events.md#matcher) for the details.

The `command` field references the name of an installation defined in [the `[[installations]]` section](#installing-auxiliary-tools) described previously. For example:

```toml
[[installations]]
name = "my-crate-hooks"
source = "cargo"
crate = "my-crate-hooks"
executable = "my-crate-hooks"
```

The hook binary receives symposium canonical JSON on stdin and writes symposium canonical JSON to stdout. Symposium handles converting to and from each agent's wire format, so a single implementation works on every agent that runs hooks. See [Writing a hook handler](./writing-a-hook-handler.md) for how to implement the binary using the `symposium-sdk` crate, and [Symposium hook events](../reference/hook-events.md) for input/output JSON schemas.

#### Agent-specific hooks

You can also provide hooks specialized for a particular agent by setting `format` to an agent name. The handler receives that agent's native wire format on stdin — giving you access to agent-specific features (e.g., Claude Code's `updatedInput`, Copilot's `modifiedArgs`). Symposium still intermediates; it just delivers in the declared format instead of converting to canonical. On agents without a matching hook, symposium falls back to delivering any symposium-format hook the plugin declares.

```toml
[[hooks]]
name = "check-usage-claude"
event = "PreToolUse"
format = "claude"
command = "my-crate-hooks"
args = ["--claude"]
```

On Claude, `check-usage-claude` fires (receives Claude's native JSON). On other agents, `check-usage` fires (receives symposium canonical JSON). See the [plugin definition reference](../reference/plugin-definition.md#hooks) for the full `[[hooks]]` manifest syntax.

### MCP servers

MCP servers expose tools and resources to agents via the [Model Context Protocol](https://modelcontextprotocol.io/). Symposium registers them into each agent's configuration during sync — you declare the server once and it works across all agents.

```toml
[[mcp_servers]]
name = "my-crate-tools"
command = "my-crate-mcp"
args = ["--stdio"]
env = []
```

`command` is written into the agent's MCP configuration as is, so it must be an absolute path or a program on the user's `PATH`. Unlike hooks, MCP servers don't use `[[installations]]`: Symposium does not install the server, so tell your users how to install it, for example in your README (`cargo install my-crate-mcp`). `args` and `env` are required, even when empty.

See the [plugin definition reference](../reference/plugin-definition.md#mcp_servers) for HTTP and SSE transports, crate filtering, and registration details.

### Subcommands

A plugin can also add commands that users and agents run as `cargo agents <name>`, backed by an installation like a hook. See the [plugin definition reference](../reference/plugin-definition.md) for the `[subcommand.<name>]` syntax.

## Step 3. Validate your plugin

Before submitting a PR to the recommendations repo, validate your entry to catch errors early: missing fields, bad crate predicates, unreachable skill paths, and crate names that don't exist on crates.io. Run this from the root of your local checkout once you've prepared your changes:

```bash
# Validate every entry in the checkout
cargo agents plugin validate .

# Skip the crates.io name check (e.g., for private crates)
cargo agents plugin validate . --no-check-crates
```

Pass the directory that contains your entry, not the entry's own directory: a directory holding a `SYMPOSIUM.toml` or `SKILL.md` is an entry, not a registry.

To try the entry before opening the pull request, copy its directory into `~/.symposium/plugins/` (a registry Symposium reads by default), run `cargo agents sync` in a project that depends on the crate, and delete the copy when you are done.

## Step 4. Try it in a project

Before publishing, check what your users will get. In a scratch project that depends on your crate by path, enable the plugin (this assumes you have run [`cargo agents init`](../reference/cargo-agents-init.md)):

```bash
cargo new try-my-crate && cd try-my-crate
cargo add my-crate --path ../my-crate
cargo agents use my-crate
cargo agents status
```

`cargo agents use` records the choice and syncs, and `cargo agents status` should list your plugin as active; if it doesn't, see [Edge cases](./supporting-your-crate.md#edge-cases). Its skills are now in each configured agent's skills directory, for example `.claude/skills/` for Claude Code.

To try a hook or subcommand before its binary is published, point the installation at your local build: drop `source` and `crate`, and set `executable` to the binary's absolute path:

```toml
[[installations]]
name = "my-crate-hooks"
executable = "/path/to/my-crate-hooks/target/debug/my-crate-hooks"
```

Then, from the scratch project, send Symposium the payload an agent would send, using a tool call your hook reacts to (this one runs `ls`). This runs the same dispatch the agent triggers, so it prints what the agent would receive:

```bash
echo '{"session_id":"test","cwd":"'"$PWD"'","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}' \
  | cargo agents hook claude pre-tool-use
```

Each agent has its own payload format; see [Agent details](../design/agent-details/index.html) for the others. If nothing happens, rerun the command with `--verbose` to see which plugins and hooks Symposium considered, and check the logs in `~/.symposium/logs/`.

When you are done, remove the test enablement with `cargo agents use my-crate --remove`. Before you publish, restore the `source = "cargo"` installation and publish the hook crate first: your users' agents install it from crates.io.

A path dependency reads your source tree, so it doesn't catch files missing from the published package; check those with `cargo package --list` (see step 2 of [Embedding skills in your crate](#embedding-skills-in-your-crate-recommended)).

Inside your crate's own repository, a `SYMPOSIUM.toml` or `skills/` directory is already active as a [workspace plugin](../workspace-skills.md#workspace-plugins) (`[package.metadata.symposium]` is not read there), which is handy while writing skills. Its hooks and MCP servers are active there too; gate a component with `predicates = ["not(workspace-member())"]` to keep it for your users only.

[rr]: https://github.com/symposium-dev/recommendations
