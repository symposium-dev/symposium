# Subcommands

A **subcommand** is a top-level `cargo agents <name>` command vended by a plugin. Subcommands are the fourth thing a plugin can contribute, alongside skills, [hooks](./hooks.md), and MCP servers. Where skills and MCP servers extend the *agent's* surface, subcommands extend `cargo agents` itself, exposing crate-aware tooling that runs on the user's machine.

The motivating use cases:

- A crate ships its own analysis binary alongside the library. The crate author wants `cargo agents <name> …` to be a discoverable entry point for agents working in projects that depend on that crate, rather than requiring users to install and remember a separate CLI.
- `crate-info` is moved out of the built-in CLI into a first-party plugin, shrinking the static command surface.
- A `[subcommand.<name>]` named after the crate is the expected convention, but is not enforced.

## Relationship to `[[installations]]`

Subcommands reuse the [installation framework](./hooks.md#installations) introduced for hooks. An installation declares *how to acquire a binary or script* (cargo install with binstall fast-path, github clone, or a path on disk), where it caches, and which `executable` or `script` to run. Subcommands reference installations by name, or declare them inline — the same shape hooks use.

This means a plugin author writes installation logic once and shares it across hooks and subcommands. Symposium owns acquisition, caching, idempotency, and post-install setup; subcommands only own dispatch.

## Manifest schema

```toml
name = "demo-plugin"
depends-on = ["example-crate"]

[[installations]]
name = "example-tool"
source = "cargo"
crate = "example-tool"
executable = "example-tool"
args = ["serve"]

[subcommand.demo]
description = "Run the demo tool"
audience = "agents"
command = "example-tool"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `description` | string | yes | Shown in `cargo agents --help`. Capped at 1024 chars. |
| `audience` | `"humans"` \| `"agents"` | no, defaults to `"agents"` | Controls grouping in `cargo agents --help`. |
| `command` | string or table | yes | A string names an `[[installations]]` entry; a table is an inline installation, promoted to a synthetic entry named after the subcommand. Same shape as `[[hooks]].command`. |
| `depends-on` | string or array | no | Subcommand-level dependency predicate, AND-combined with the plugin-level `depends-on`. |

Reserved names that cannot be used as subcommand keys: `init`, `sync`, `hook`, `plugin`, `crate-info`, `help`. A plugin cannot shadow a built-in.

The TOML key is singular (`[subcommand.<name>]`), matching the natural read of a TOML table. The internal field on `Plugin` is plural (`subcommands`).

### Inline form

For one-off subcommands the inline form avoids a separate `[[installations]]` block:

```toml
[subcommand.demo]
description = "..."
command = { source = "cargo", crate = "example-tool", executable = "example-tool", args = ["serve"] }
```

The inline table is promoted to a synthetic installation named after the subcommand and resolved through the same pipeline.

## Pass-through contract

Symposium does not own the subcommand's argument grammar. The plugin's binary owns its own `--help`, validation, and exit codes. What symposium contributes is mechanical:

1. Name registration and lookup.
2. Workspace-aware filtering (the subcommand only appears for projects matching the plugin's dependency predicates).
3. A short description shown in `cargo agents --help`.
4. Resolution of `command` through the installation pipeline to a concrete `(executable, base_args)`.
5. Forwarding the user's trailing CLI args verbatim, appended after the installation's `args`.

This boundary keeps the manifest small, keeps plugins authoritative about their CLI, and avoids inventing a symposium-specific options DSL that would drift from each plugin's real interface.

## Dispatch

`cargo-agents`'s top-level CLI uses clap's `allow_external_subcommands`: unknown subcommands are not errors but are routed to a catch-all variant. The binary then:

1. Loads the plugin registry and the active workspace's crates.
2. Walks active plugins for one whose `subcommands` map contains the typed name *and* whose subcommand-level `depends-on` predicate (if any) also matches.
3. Resolves the subcommand's `command` through the installation pipeline — acquiring the binary if it isn't already cached, running any `install_commands`, processing `requirements`.
4. Execs the resolved `(executable, base_args ++ user_args)`, inheriting stdio.
5. Returns the child's exit code as the `cargo agents` exit code. A signal-killed child becomes a generic failure.

Argument forwarding uses a structured `Vec`, not `sh -c`. User-supplied argv is preserved exactly — spaces, quotes, and shell metacharacters in args are not re-tokenized. This matters more for subcommands than for hooks (whose input arrives over stdin as JSON).

Script-mode installations (`script = "..."`) are still invoked through `sh <path>`, mirroring hook dispatch. That path is not cross-platform on Windows and is tracked as a follow-up for both hooks and subcommands; in the meantime, plugin authors who need Windows support should use `executable = "..."` instead.

If no plugin matches the typed name, dispatch fails with a clear error pointing to `cargo agents --help`. If a matching subcommand exists but installation fails, the installation layer's error is propagated as-is.

## Workspace filtering

Plugin filtering is workspace-aware in two places: help rendering and dispatch.

**Inside a Cargo workspace.** Symposium reads the workspace's resolved dependencies. A subcommand appears in `cargo agents --help` and is dispatchable only if both the plugin-level and subcommand-level `depends-on` predicates match. Built-in subcommands always appear.

**Outside a Cargo workspace** (no discoverable `Cargo.toml` upward). Only built-ins and plugins with `depends-on = ["*"]` appear. Invoking a crate-specific subcommand from outside a workspace produces an error explaining which crate it needs.

This rule keeps `cargo agents --help` outside a workspace limited to globally-applicable commands, rather than listing every installed plugin.

## Help text grouping

`cargo agents --help` is rendered in two sections:

- **Commands for humans** — operational commands a user runs themselves: `init`, `plugin`, `self-update`, `sync`, plus any plugin-vended subcommand with `audience = "humans"`.
- **Commands for agents** — discovery and analysis tools for the agent to invoke: `crate-info` and plugin-vended subcommands with `audience = "agents"` (the default).

The default of `audience = "agents"` reflects the expected shape of plugin-vended commands: most are analysis or context-fetching tools surfaced to agents, not workflows for humans. The exceptional case explicitly opts in.

For this grouping to be useful, `crate-info` is no longer hidden — it's a discoverable agent tool. `hook` remains hidden; it's an internal protocol entry point, not an end-user surface.

The renderer reads the active plugin registry filtered by workspace, so the help output adapts to the project the user is standing in.

`--help`, `-h`, the bare `help` keyword, and an empty invocation are intercepted after clap parses and routed to this renderer; `help` is never listed as its own command. A `<built-in> --help` instead shows that command's own help (re-rendered from clap), and a plugin-vended `<name> --help` is forwarded to the plugin's binary, which owns its `--help`.

## Agent discovery

`cargo agents --help` is a *pull* surface — an agent only sees the crate-aware subcommands if it already knows to run it. To *push* that affordance, the built-in `SessionStart` hook injects a one-line hint suggesting `cargo agents --help` whenever the active workspace exposes at least one applicable plugin-vended subcommand. The trigger reuses the same workspace-filtered set as the help renderer (`applicable_subcommands`), so the hint stays silent in projects with nothing to discover.

The hint shares `SessionStart`'s `additionalContext` with the [update nudge](./hook-flow.md); each fragment is computed independently, and the discovery hint is not gated behind the update-check throttle. Agents without hook registration (OpenCode, Goose) don't receive it; for them `cargo agents --help` is the only discovery surface.

## Audience as metadata, not enforcement

`audience` controls help-text grouping only. It does not gate dispatch. A user can type `cargo agents <agent-audience-subcommand>` directly and it will run. The intent is to keep the discovery surface uncluttered for humans, not to lock anyone out.

## Conflict resolution

Two plugins may declare the same subcommand name. Rather than silently picking one, dispatch fails with an error listing every plugin that defined the name, leaving the user to disambiguate (typically by tightening one of the plugin's `depends-on` predicates or removing one of the plugin sources).

The strict-error stance trades silence for clarity: subcommand names tend to mirror crate names (which are unique on crates.io), so a collision usually signals a real configuration mistake rather than an intended override.

Namespacing (`cargo agents <plugin>:<name>`) is not implemented; it can be revisited if a real conflict pattern emerges.

## What plugins own vs. what symposium owns

| Concern | Owned by |
|---------|----------|
| Subcommand name | Manifest |
| Short description | Manifest |
| `audience` | Manifest |
| Argument grammar, flags, `<subcommand> --help` | Plugin's binary |
| Argument validation | Plugin's binary |
| Exit codes | Plugin's binary, propagated by symposium |
| Binary acquisition, caching, post-install setup | Shared installation framework |
| Workspace-aware filtering | Symposium |
| Resolution of `command` → `(executable, args)` | Shared installation framework |
| Stdio forwarding | Symposium (inherited) |
| `cargo agents --help` rendering | Symposium |
| Conflict resolution | Symposium |

