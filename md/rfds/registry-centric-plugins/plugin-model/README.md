# Plugin model

## TL;DR

- A plugin is a directory. Every directory is a valid plugin — no manifest required.
- An optional `Symposium.toml` provides explicit configuration. If absent, an empty one is synthesized.
- Defaults apply to every plugin: `skills/` and `.agents/skills/` are discovered as skill directories.
- Plugins can declare chained plugins (additional plugins to load when activated).
- Predicates gate activation, not installation.

## Motivation

The old plugin model was built around explicit manifests in "plugin source" directories. This made it hard for crate authors to ship skills without learning a new configuration system. The new model inverts the default: everything is a plugin, configuration is optional, and conventions do the heavy lifting.

## Change in a nutshell

A plugin directory with nothing but a `skills/` subdirectory:

```
my-plugin/
└── skills/
    └── usage-guide/
        └── SKILL.md
```

This is a valid, complete plugin. No `Symposium.toml` needed. Symposium synthesizes an empty manifest and applies defaults, which discovers the skill.

Adding a `Symposium.toml` lets you control behavior — add predicates, declare hooks, reference binaries, suppress defaults, or chain other plugins:

```toml
# Symposium.toml
[depends-on]
cargo = { tokio = "1" }

[[hooks]]
event = "PreToolUse"
command = "my-linter"

[[plugins]]
source.cargo = { tokio-extras = "*" }
```

## Detailed plans

### What is a plugin?

A plugin is a directory. That's it. The directory may contain:

- `Symposium.toml` — optional manifest
- `skills/` — conventional skill directory (exposed to workspace and dependency consumers)
- `.agents/skills/` — conventional skill directory (workspace-only)
- Any other files (scripts, assets, etc. referenced by hooks or MCP servers)

### Synthesized manifest

When a directory has no `Symposium.toml`, Symposium behaves as if an empty one exists. This empty manifest still triggers default behavior (see below).

### `Symposium.toml` structure

```toml
# Predicates gating activation
predicates = ["workspace()", "file-exists(build.rs)"]

# Shorthand: depends-on reuses the PM's resolve format
[depends-on]
cargo = { tokio = "1", serde = "1" }

# Suppress defaults
[defaults]
skills = false

# Skills (beyond those discovered by convention)
[[skills]]
source.path = "extra-skills/advanced"
predicates = ["env(ADVANCED_MODE=1)"]

# Hooks
[[hooks]]
event = "PreToolUse"
command = "my-linter"
args = ["--strict"]

[[hooks]]
event = "SessionStart"
command = "my-greeter"

# MCP servers
[[mcp]]
name = "my-server"
command = "my-mcp-binary"
args = ["serve"]

# Chained plugins — loaded when this plugin activates
[[plugins]]
source.cargo = { tokio-extras = "*" }

[[plugins]]
source.git = { url = "github.com/org/helpers", branch = "main" }

# Installable content (binaries referenced by hooks/MCP servers)
[[installable]]
name = "my-linter"
source.cargo = { my-linter-crate = "1.0" }
```

### Agentic extensions

`Symposium.toml` files contain the following kinds of content:

* `[[plugins]]` defines a set of additional *chained plugins*. If a plugin X defines a chained plugin Y, then whenever X is loaded, Y will be loaded.
* `[[skills]]` identifies directories where we should search for skills. Any skills found there will be installed into the user's workspace in the appropriate place(s) for the agent(s) they've selected.
* `[[mcp]]` identifies MCP servers.
* `[[hooks]]` identifies hooks. Symposium allows you to define vendor-neutral hooks that work for any vendor or vendor-specific hooks that target a particular agent (e.g., Claude Code or Codex).
* `[[installable]]` identifies installable content, which can be referenced by MCP servers or hooks (which need an executable). An easy option is to package your content as a cargo package that will be cargo-install'd and managed by Symposium, but there are other options.

### Default content

Plugins have default content added automatically unless disabled via `[defaults]`. Currently we have one default, `defaults.skills = (true|false)`. Assuming the default is not set to false, the following is added to the plugin:

```toml
[[skills]]
source.path = "skills"

[[skills]]
predicates = ["workspace()"]
source.path = ".agents/skills"
```

These defaults establish the skills conventions:
- `skills/` is exposed to anyone who depends on the crate (no predicate gate).
- `.agents/skills/` is only exposed when working directly in the workspace (gated by `workspace()`).

### Predicates

The plugin itself and each of its subsections can be gated with a `predicates = [...]` field. When a plugin is installed, the content is only *activated* if the predicate matches.

Common predicates:

* `workspace()` — true if this plugin is part of the active workspace
* `used()` — true if this plugin was explicitly used by the user
* `workspace-dependency()` — true if plugin is a dependency of some project in the current workspace
* `depends-on(pm, name, version)` — true if the workspace depends on this package
* `env(FOO=BAR)` — true if the environment variable is set to the given value
* `file-exists(path)` — true if the given file exists relative to workspace root
* `shell(command)` — true if the command exits with code 0
* `workspace-directory(path)` — true if the workspace is a subdirectory of the given path
* `not(p)`, `any(p, ...)`, `all(p, ...)` — combinators

The `[depends-on]` shorthand reuses the PM's `resolve` format:

```toml
[depends-on]
cargo = { tokio = "1", serde = "1" }
```

This is equivalent to `predicates = ["depends-on(cargo, tokio, 1)", "depends-on(cargo, serde, 1)"]`.

Predicates can appear at any level (plugin, skill, hook, MCP server). A predicate on a plugin gates all its direct contents. Chained plugins have their own predicates and are evaluated independently.

### Chained plugins

A plugin can declare additional plugins to be loaded when it activates:

```toml
[[plugins]]
source.cargo = { serde-extras = "*" }

[[plugins]]
source.path = { path = "./sub-plugin" }
```

Chaining is an *activation-time* relationship: when this plugin becomes active, also load these. Chained plugins:
- Are fetched and cached transitively (installing A also fetches A's chained plugins)
- Have their own predicates (they may not activate even if the parent does)
- Are independent after loading

Use chaining when a library crate wants agent support but ships it in a separate package for release-cycle independence.

### Installed vs. active

| State | Meaning | Where |
|-------|---------|-------|
| Installed | Content is in cache, ready to activate | `~/.symposium/cache/` |
| Active | Predicates pass, content wired into agent dirs | `.claude/skills/`, etc. |
| Inactive | Installed but predicates don't pass | Cache only |

A plugin transitions between active and inactive as workspace state changes (e.g., adding a dependency). No re-fetch needed.

## Frequently asked questions

### Why is every directory a plugin?

It makes the cargo PM simple: every crate is a plugin, no detection heuristic needed. Most crates won't have any plugin content (no `skills/`, no `Symposium.toml`), so they result in empty plugins that are effectively no-ops.

### What happened to "plugin sources"?

Gone. In the old model, `[[plugin-source]]` pointed at directories that *contained* plugins. Now there's just plugins — and plugins can chain other plugins.

### Can a plugin contain sub-directories that are also plugins?

Only via explicit `[[plugins]]` with `source.path`. We don't recursively scan for nested `Symposium.toml` files.

### What if `skills/` exists but I don't want it discovered?

```toml
[defaults]
skills = false
```

## Implementation plan and status

### Step 1: Plugin struct and manifest parsing

Define the `Plugin` struct, parse `Symposium.toml`, synthesize empty manifests for directories without one.

- [ ] PR: plugin struct + TOML parsing

### Step 2: Default application

Implement skill discovery from `skills/` and `.agents/skills/`. Suppression via `[defaults]`.

- [ ] PR: plugin defaults

### Step 3: Predicates on plugins

Evaluate predicates at the plugin level and per-construct level. Gate activation. Implement the `[depends-on]` shorthand.

- [ ] PR: predicate evaluation

### Step 4: Chained plugins

Parse `[[plugins]]` entries, resolve via PMs, fetch transitively, evaluate independently.

- [ ] PR: chained plugin loading

### Step 5: Integration with sync

Wire the new plugin model into the sync pipeline: iterate installed plugins, evaluate predicates, sync active content to agent directories.

- [ ] PR: sync integration
