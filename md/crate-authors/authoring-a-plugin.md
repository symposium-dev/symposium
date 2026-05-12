# Authoring a plugin

Symposium lets you ship skills, hooks, and MCP servers that are automatically loaded when a user's project depends on your crate. This page walks through the options, starting with the simplest.

## Skills

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

### Embedding skills in your crate (recommended)

If you maintain the crate, we recommend shipping skills directly in your source tree. Place skill directories under `skills/`:

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

To make Symposium aware of your skills, open a PR adding a plugin manifest to the [central recommendations repository][rr]. This is a small TOML file that tells Symposium to look inside your crate's source:

```toml
name = "my-crate"
crates = ["my-crate"]

[[skills]]
source = "crate"
```

Symposium fetches the crate source (from the local cargo cache or crates.io) and discovers the skills. Users always get skills matching the version they have installed.

We recommend placing skills in `skills/`, but if you prefer a different directory, you can use `source.crate_path`:

```toml
[[skills]]
source.crate_path = ".symposium/skills"
```

### Standalone skills (for third-party crates)

You can also upload skills as standalone directories — without embedding them in the crate source. This is the right approach when you're writing skills for a crate you don't maintain. We prefer crates to ship their own skills, but some crates may not want to or may not be actively maintained. We accept skills for those crates to help bootstrap the ecosystem.

Standalone skills are uploaded directly to the [recommendations repo][rr] so that we can vet them. The expected directory structure is:

```
crate-name/
    skill-name/
        SKILL.md
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

### Why is the central repository required?

We currently require an entry in our central [recommendations repository][rr] before Symposium will install skills. This protects against malicious skills (e.g., from typosquatting crates) and lets us centrally yank a plugin that proves problematic. Once Symposium has reached a steady state and we have established security protocols we are comfortable with, we expect to lift this requirement.

## Plugins

If you need more than skills — hooks, MCP servers, or skills hosted in a separate git repo — you can upgrade to a full plugin by adding a `SYMPOSIUM.toml` manifest.

For example, if you already have standalone skills in the recommendations repo:

```
my-crate/
    basics/
        SKILL.md
    advanced-patterns/
        SKILL.md
```

You can add a `SYMPOSIUM.toml` alongside them:

```
my-crate/
    SYMPOSIUM.toml
    basics/
        SKILL.md
    advanced-patterns/
        SKILL.md
```

With the manifest:

```toml
name = "my-crate"
crates = ["my-crate"]

[[skills]]
source.path = "."
```

The `source.path = "."` tells Symposium to look for skills in the same directory as the manifest. Now that you have a plugin, you can add hooks and MCP servers.

### Hooks

Hooks run when the AI performs certain actions, like invoking a tool or starting a session. Add a `[[hooks]]` entry to your manifest:

```toml
[[hooks]]
name = "check-usage"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/check.sh"
```

The hook receives JSON on stdin describing the tool invocation and can return guidance or block the action. See the [plugin definition reference](../reference/plugin-definition.md#hooks) for supported events, wire formats, and semantics.

### MCP servers

MCP servers expose tools and resources to agents via the [Model Context Protocol](https://modelcontextprotocol.io/). Symposium registers them into each agent's configuration during sync.

```toml
[[mcp_servers]]
name = "my-crate-tools"
command = "my-crate-mcp-server"
args = ["--stdio"]
env = []
```

See the [plugin definition reference](../reference/plugin-definition.md#mcp_servers) for HTTP and SSE transports, crate filtering, and registration details.

## Validation

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
