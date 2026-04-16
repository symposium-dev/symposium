# Creating a plugin

A plugin is a TOML manifest that bundles skills and hooks together. You need a plugin if you want to publish hooks, or if you want to host skills from your own repository rather than contributing them to the recommendations repo.

## What's in a plugin

A plugin manifest has three parts:

1. **A name** — identifies the plugin in logs and CLI output.
2. **Skill groups** (`[[skills]]`) — each one points at a directory of skills and declares which crates they're for.
3. **Hooks** (`[[hooks]]`) — commands that run in response to agent events.

Skills and hooks are both optional — a plugin can have just skills, just hooks, or both.

## Skill groups

Each `[[skills]]` entry declares which crates the skills apply to and where to find them. The source can be a local path or a git URL:

```toml
name = "widgetlib"

# Skills live next to the manifest
[[skills]]
crates = ["widgetlib"]
source.path = "skills"
```

```toml
name = "widgetlib"

# Skills are fetched from a GitHub repository
[[skills]]
crates = ["widgetlib"]
source.git = "https://github.com/org/widgetlib/tree/main/symposium/skills"
```

Use `source.path` when skills are on the local machine or in the same repository as the manifest. Use `source.git` when they're hosted elsewhere — Symposium downloads and caches them automatically.

**Warning:** Skills in a plugin will only be fetched if the `crates` list matches, so you must include it.

You can have multiple skill groups in one plugin, each targeting different crates or versions:

```toml
name = "widgetlib"

[[skills]]
crates = ["widgetlib=1.0"]
source.path = "skills/v1"

[[skills]]
crates = ["widgetlib=2.0"]
source.path = "skills/v2"
```

## Hooks

Hooks let your plugin respond to agent events. See [Publishing hooks](./publishing-hooks.md) for details.

```toml
[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/check-widget.sh"
```

## Where to put your plugin

You have two options:

1. **In your crate's repository** — add a `symposium.toml` at the root (or a subdirectory). The recommendations repo can then reference it via a git URL.
2. **In the recommendations repo** — submit a PR to [symposium-dev/recommendations](https://github.com/symposium-dev/recommendations) with your plugin manifest.

## Validation

```bash
cargo agents plugin validate path/to/symposium.toml
```

This parses the manifest and reports any errors. Use `--check-crates` to also verify that crate names exist on crates.io.

## Reference

See the [Plugin definition reference](../reference/plugin-definition.md) for the full manifest format.
