# Authoring a plugin

Symposium lets you ship skills, hooks, and MCP servers that are automatically loaded when a user's project depends on your crate. This page walks through how to create a plugin and configure each extension type.

## Step 1. Create a `SYMPOSIUM.toml` manifest

Every plugin starts with a `SYMPOSIUM.toml` manifest uploaded to the [central recommendations repository][rr]. The manifest declares your plugin's name, which crates it applies to, and what extensions it provides.

```toml
# `my-crate/SYMPOSIUM.toml` on the symposium-dev/recommendations repository
name = "my-crate"
crates = ["my-crate"]
```

The `crates` field controls when the plugin is active — it will only load for projects that depend on the listed crates. Use `["*"]` to apply to all projects.

See the [plugin definition reference](../reference/plugin-definition.md) for the full manifest schema.

### Why is the central repository required?

We currently require an entry in our central [recommendations repository][rr] before Symposium will install a plugin. This protects against malicious plugins (e.g., from typosquatting crates) and lets us centrally yank a plugin that proves problematic. Once Symposium has reached a steady state and we have established security protocols we are comfortable with, we expect to lift this requirement.

## Step 2. Add skills, hooks, and/or MCP servers

With your manifest in place, you can add any combination of the extension types below.

### Skills

Skills are guidance documents that teach AI assistants how to use a crate. Each skill is a directory containing a `SKILL.md` file with YAML frontmatter and a markdown body:

```markdown
---
name: my-crate-basics
description: Basic guidance for my-crate usage
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

##### 2. Add `source = "crate"` to your manifest

```toml
# `my-crate/SYMPOSIUM.toml` on the symposium-dev/recommendations repository
name = "my-crate"
crates = ["my-crate"]

[[skills]]
source = "crate"
```

Symposium fetches the crate source (from the local cargo cache or crates.io) and discovers skills in the `skills/` directory.

##### Prefer a directory other than `skills/`?

Add `[package.metadata.symposium]` to your crate's `Cargo.toml` to specify a custom path:

```toml
# In your crate's Cargo.toml
[[package.metadata.symposium.skills]]
path = "docs/agent-skills"
```

When no metadata section is present, Symposium defaults to the `skills/` directory. See [Supporting your crate](./supporting-your-crate.md) for the full metadata schema including redirects to other crates.

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
crates = ["my-crate"]

[[skills]]
source.path = "."
```

Standalone skills **must** include `crates` in their frontmatter so Symposium knows which crate they apply to:

```markdown
---
name: widgetlib-basics
description: Basic guidance for widgetlib usage
crates: widgetlib=1.0
---

Guidance body here.
```

#### Skills from a git repository

Symposium also supports fetching skills from a GitHub URL:

```toml
[[skills]]
source.git = "https://github.com/org/my-crate/tree/main/symposium/skills"
```

This is useful for hosting skills in a dedicated repository or a subdirectory of a monorepo. Note that the central recommendations repository does not currently accept `source.git` entries by policy — use `source = "crate"` or `source.path` for submissions there.

### Installing auxiliary tools

An **installation** tells symposium how to obtain a binary that your hooks or MCP servers will run. The recommended approach is a `cargo` installation, which installs a crate binary from crates.io:

```toml
[[installations]]
name = "my-crate-hooks"
source = "cargo"
crate = "my-crate-hooks"
executable = "my-crate-hooks"
```

Symposium caches the binary under `~/.symposium/cache/`. Binaries are updated automatically when new versions are available on [crates.io](https://crates.io/).

See the [plugin definition reference](../reference/plugin-definition.md#installations) for other installation sources (GitHub repositories, local paths) and advanced options like `install_commands`.

### Hooks

Hooks run when the AI performs certain actions — invoking a tool, starting a session, or submitting a prompt. They receive JSON on stdin describing the event and can return guidance, inject context, or block the action.

Every agent varies in the specifics of what hooks it offers and how those hooks are configured. Symposium allows you to provide agent-specific hook handlers, but we recommend instead using a *Symposium hook* handler, which is portable across all agents.

#### Symposium hooks (portable across agents)

To define a Symposium hook handler you add a `[[hooks]]` section. This defines the command to run as well as the events it expects and other filters. 

```toml
[[hooks]]
name = "check-usage"
event = "PreToolUse"
matcher = "Bash"
command = "my-crate-hook-command"
```

The `command` field references the name of an installation defined in [the `[[installations]]` section](#installations) described previously. For example:

```toml
[[installations]]
name = "my-crate-hook-command"
source = "cargo"
crate = "my-crate-hooks"
executable = "my-crate-hooks"
```

The hook binary receives symposium canonical JSON on stdin and writes symposium canonical JSON to stdout. Symposium handles converting to and from each agent's wire format, so a single implementation works across all supported agents. See [Writing a hook handler](./writing-a-hook-handler.md) for how to implement the binary using the `symposium-hook` crate, and [Symposium hook events](../reference/hook-events.md) for input/output JSON schemas.

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

An MCP server typically uses the same installation as your hooks:

```toml
[[installations]]
name = "my-crate-mcp"
source = "cargo"
crate = "my-crate-mcp"
executable = "my-crate-mcp"

[[mcp_servers]]
name = "my-crate-tools"
command = "my-crate-mcp"
args = ["--stdio"]
```

See the [plugin definition reference](../reference/plugin-definition.md#mcp_servers) for HTTP and SSE transports, crate filtering, and registration details.

## Step 3. Validate your plugin

Before submitting a PR, validate your plugin or skill directory to catch errors early — missing fields, bad crate predicates, unreachable skill paths, and crate names that don't exist on crates.io. You can run this on your local checkout of the recommendations repo once you've prepared your changes:

```bash
# Validate a plugin manifest
cargo agents plugin validate path/to/SYMPOSIUM.toml

# Validate a directory of standalone skills
cargo agents plugin validate path/to/skill-directory/

# Skip the crates.io name check (e.g., for private crates)
cargo agents plugin validate path/to/SYMPOSIUM.toml --no-check-crates
```

[rr]: https://github.com/symposium-dev/recommendations
