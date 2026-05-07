# Creating a plugin

A **symposium plugin** collects together all the extensions offered for a particular crate. Plugins can references skills, hooks, MCP servers, and other things that are relevant to your crate. These extensions can be packaged up as part of the plugin or the plugin can contain pointers to external repositories; the latter is often easier to update in a centralized fashion.

Plugins are needed when you want to add mechanisms beyond skills, like hooks or MCP servers. If all you want to do is upload a skill, you can [publish a standalone skill instead](./publishing-skills.md).

## Plugin structure

A plugin is structured as a directory that contains a `SYMPOSIUM.toml` manifest file:

```
my-crate-plugin/
    SYMPOSIUM.toml
    ... // potentially other stuff here
```

The `SYMPOSIUM.toml` plugin manifest has these main sections:

```toml
name = "my-crate-plugin"

# The `crates` field defines when the plugin applies. Use the special `crates = ["*"]` form to write a plugin that ALWAYS applies.
crates = ["my-crate"]

# Skills grouped by crate and version.  Skills can be located as a reference to another git repository.
[[skills]]
crates = ["my-crate=2.0"]
source.git = "https://github.com/org/my-crate/tree/main/skills/v2"

# Skills can also be located by a path within the plugin itself.
[[skills]]
crates = ["my-crate=1.0"]
source.path = "skills/v1"

# A reusable, named installation. Hooks reference these by name.
[[installations]]
name = "rg"
source = "cargo"
crate = "ripgrep"
version = "13.0.0"
binary = "rg"

# Hooks for agent event callbacks
[[hooks]]
name = "validate-usage"
event = "PreToolUse"
# Run a local script bundled with the plugin
command = { source = "local", command = "./scripts/validate.sh" }

[[hooks]]
name = "ripgrep-hook"
event = "PreToolUse"
# Run the cargo-installed binary by name, with hook-level args
command = "rg"
args = ["--version"]
# Install a separate helper before running the hook
requirements = [{ source = "cargo", crate = "my-tool", version = "0.2.1" }]

# MCP servers for tool integration
[[mcp_servers]]
name = "my-crate-tools"
command = "my-crate-mcp-server"
```

## Publishing plugins

**The most common way to publish a plugin is to upload the plugin directory to our [central recommendations repository][rr].** If you do this, we recommend keeping your skills and other resources in your own github repo, so that you can update them without updating the central repository.

[rr]: https://github.com/symposium-dev/recommendations

**We expect in the future to make it possible to add plugins directly into your crate definition**, but that is not currently possible.

**Plugins can also be added to a [custom plugin source][ps].** This is useful when you are defining rules for crates specific to your company or customized plugins that are tailored to the way that your project uses a crate. In this case, it's often convenient to package up the skills, MCP servers, etc together with the plugin.

[ps]: ../custom-plugin-source.md

## Example: plugin for widgetlib with remote skills

Consider a `widgetlib` crate that wants to keep skills in its own repository but still be discoverable through Symposium.

This would be done by first uploading a plugin to the [symposium-dev/recommendations][rr] repository:

```text
widgetlib/
    SYMPOSIUM.toml
```

This `SYMPOSIUM.toml` file would list out the skills as pointers to a remote repository. In this case, let's say there are two groups of skills, one for v1.x and one for v2.x, and those skills are hosted on the repo `widgetlib/skills`:

```toml
name = "widgetlib"

[[skills]]
crates = ["widgetlib=1.0"]
source.git = "https://github.com/widgetlib/skills/tree/main/symposium/skills/v1"

[[skills]]
crates = ["widgetlib=2.0"]
source.git = "https://github.com/widgetlib/skills/tree/main/symposium/skills/v2"
```

In this example, the `crates` criteria is placed on the skill groups. This is because the skill groups have distinct versions. If there were a `crates` declaraton at the top-level, then both the plugin crates definition *and* the skills definition must match (in addition to any `crates:` defined in the skill metadata!).

The `widgetlib/skills` repo then would look like this:

```text
v1/
    basics/
        SKILL.md
v2/
    basics/
        SKILL.md
    migration/
        SKILL.md
```

Given this setup, each time a new commit is pushed to `widgetlib/skills`, users' skills will automatically be updated to match.

## Details on how to define a plugin

See the reference for [the precise specification of what is allowed in a plugin definition](../reference/plugin-definition.md). This includes the details of how to define other plugin content beyond skills, such as hooks and MCP servers.

## Validation and testing

You can test that a plugin directory has the correct structure using the `cargo agents` CLI:

```bash
# Validate your plugin manifest
cargo agents plugin validate path/to/SYMPOSIUM.toml

# Check that crate names exist on crates.io
cargo agents plugin validate path/to/SYMPOSIUM.toml --check-crates
```
