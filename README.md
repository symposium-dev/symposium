# Symposium

<div align="center" style="text-align: center;">
  <img src="https://raw.githubusercontent.com/symposium-dev/symposium/refs/heads/main/md/artwork/symposium5_vase-ferris.png" alt="Symposium logo" width="300"/>
</div>

<p align="center" style="text-align: center;"><em>Collaborative AI built collaboratively</em></p>

<p align="center">
  <a href="https://crates.io/crates/symposium"><img src="https://img.shields.io/crates/v/symposium.svg" alt="crates.io"/></a>
  <a href="https://symposium.dev"><img src="https://img.shields.io/badge/docs-symposium.dev-1f6feb" alt="Documentation"/></a>
  <a href="./LICENSE.txt"><img src="https://img.shields.io/crates/l/symposium.svg" alt="License: MIT OR Apache-2.0"/></a>
</p>

Symposium scans a Rust workspace, matches plugins against its dependency graph, and installs the corresponding **skills**, **hooks**, and **MCP servers** into the AI coding agents you use. Extensions are authored by crate authors and distributed through plugin sources, so an agent working in your project sees guidance specific to the crates that project depends on.

It ships as `cargo agents`, a Cargo subcommand. One agent-agnostic configuration is translated into the per-agent files each supported agent expects, so different people on the same project can use different agents.

## Contents

- [Requirements](#requirements)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
- [How matching works](#how-matching-works)
- [Commands](#commands)
- [Configuration](#configuration)
- [Supported agents](#supported-agents)
- [For crate authors](#for-crate-authors)
- [Documentation](#documentation)
- [Community](#community)

## Requirements

- A Rust toolchain with `cargo` on `PATH`. Symposium runs `cargo metadata` to read the dependency graph and `cargo install` to update itself.
- At least one [supported agent](#supported-agents) installed.
- Linux, macOS, or Windows.

## Installation

Install with [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall) (prebuilt binary) or `cargo install` (from source):

```bash
cargo binstall symposium   # or: cargo install symposium
```

Both provide the `cargo-agents` binary, invoked as `cargo agents`.

## Quick start

Run `init` once per machine to record which agents you use and register hooks:

```bash
cargo agents init
```

`init` prompts for your agents (select one or more) and for hook scope:

```text
Which agents do you use? (space to select, enter to confirm):
> [ ] Claude Code
  [x] Codex CLI
  [ ] GitHub Copilot
  [ ] Gemini CLI
  [ ] Goose
  [x] Kiro
  [x] OpenCode
```

- **Global** hook scope (default) registers Symposium for the selected agents in your home directory, so it activates in every Rust project.
- **Project** hook scope writes hooks into the current project only; run `cargo agents sync` once in each project to activate it there.

After setup, start your agent inside a Rust workspace as usual. With `auto-sync` enabled (the default), Symposium re-scans dependencies on each hook invocation and keeps the installed skills current. You can also run a sync explicitly:

```bash
cargo agents sync
```

## Concepts

| Term | Meaning |
|------|---------|
| **Skill** | A `SKILL.md` file (and its directory) containing guidance the agent loads as context. |
| **Hook** | A check or transformation that runs when the agent performs an action, such as writing code or running a command. |
| **MCP server** | Tools and resources exposed to the agent over the [Model Context Protocol](https://modelcontextprotocol.io). |
| **Plugin** | A manifest that bundles skills, hooks, MCP servers, and subcommands, each gated by crate predicates. |
| **Plugin source** | A directory or git repository containing plugin manifests. The central [recommendations repository][rr] and `~/.symposium/plugins/` are enabled by default. |

## How matching works

Symposium reads the workspace dependency graph and evaluates each plugin's and skill's predicates against it. An extension is installed only when its predicates match.

- The `depends-on` field matches by crate name and version requirement (`serde`, `serde >= 1.0`, `*`).
- The `predicates` field uses function-call syntax (`depends-on(...)`, `shell(...)`, `path_exists(...)`, `env(...)`), combined with `not`, `any`, and `all`.

When a skill group declares `source = "crate"`, Symposium fetches the matched crate's source (from the local path, the cargo registry cache, or crates.io), reads `[package.metadata.symposium]` from its `Cargo.toml` to locate the skills, and follows crate-to-crate redirects. See the [predicates reference](https://symposium.dev/reference/predicates.html).

## Commands

| Command | Description |
|---------|-------------|
| `cargo agents init` | Record your agents and register hooks (writes `~/.symposium/config.toml`). |
| `cargo agents sync` | Match plugins against workspace dependencies and install/refresh skills. |
| `cargo agents plugin` | Inspect and manage plugin sources. |
| `cargo agents self-update` | Install the latest published version. |
| `cargo agents crate-info` | Resolve and fetch a crate's source (used by agents). |

Global flags include `-v`/`--verbose` (decision trace), `--json` (structured output), and `-q`/`--quiet`. See the [command reference](https://symposium.dev/reference/cargo-agents.html).

## Configuration

Configuration is a single user-wide file at `~/.symposium/config.toml`, created by `init`. The top-level keys:

| Key | Default | Description |
|-----|---------|-------------|
| `auto-sync` | `true` | Run `cargo agents sync` automatically during hook invocations. |
| `agents-syncing` | `true` | Mirror user-authored skills from `.agents/skills/` into agents that use their own skill directory. |
| `hook-scope` | `"global"` | Install hooks in the home directory (`global`) or the project (`project`). |
| `auto-update` | `"on"` | `off`, `warn` (notify when a newer version exists), or `on` (install and re-exec). |

`[[agent]]` entries list your agents, `[[plugin-source]]` adds git or local plugin sources, and `[defaults]` toggles the two built-in sources. User data lives under `~/.symposium/` (overridable via `SYMPOSIUM_HOME` or the XDG variables). See the [configuration reference](https://symposium.dev/reference/configuration.html).

## Supported agents

Every agent receives skill installation. Hook registration is available for a subset; OpenCode and Goose are skills-only.

| Agent | Skill directory | Hooks |
|-------|-----------------|:-----:|
| Claude Code | `.claude/skills/` | Yes |
| GitHub Copilot | `.agents/skills/` | Yes |
| Gemini CLI | `.agents/skills/` | Yes |
| Codex CLI | `.agents/skills/` | Yes |
| Kiro | `.kiro/skills/` | Yes |
| OpenCode | `.agents/skills/` | No |
| Goose | `.agents/skills/` | No |

## For crate authors

If you maintain a Rust crate, you can ship skills, hooks, and MCP servers so AI-assisted users of your library get guidance matched to the exact version they depend on. Add a `skills/` directory to your crate, control its layout with `[package.metadata.symposium]` in `Cargo.toml`, and register a plugin manifest in the [recommendations repository][rr].

See [Supporting your crate](https://symposium.dev/crate-authors/supporting-your-crate.html) and [Authoring a plugin](https://symposium.dev/crate-authors/authoring-a-plugin.html).

## Documentation

Full documentation lives at **[symposium.dev](https://symposium.dev)**: installation, configuration, plugin and hook authoring, supported-agent details, and design notes.

## Community

Symposium is open source under [MIT or Apache-2.0](./LICENSE.txt). We welcome [contributors](https://symposium.dev/design/welcome.html) and maintain a [code of conduct](./CODE_OF_CONDUCT.md).

[rr]: https://github.com/symposium-dev/recommendations
