# Getting Started

## Install

`cargo-agents` requires the [Rust toolchain](https://rustup.rs/). Install it with:

```bash
cargo install cargo-agents
```

Or, for a faster prebuilt binary:

```bash
cargo binstall cargo-agents
```

## Step 1: User-wide setup

The preferred setup for `cargo-agents` lets each user pick their own agent. To configure your user-wide agent, run the following from any directory:

```bash
cargo agents init --user
```

This will ask you which agent you use (e.g., Claude Code, Cursor) and store your preference in `~/.cargo-agents/config.toml`. Where applicable, it also registers a global hook so that your agent automatically picks up project extensions on startup.

If you're working in a project that already uses `cargo-agents`, this is all you have to do — when you open the project in your agent, everything will be set up automatically.

## Step 2: Project setup

If you're setting up a project for the first time, navigate to your Rust project and run:

```bash
cargo agents init --project
```

This will ask you a few questions to help you get set up. It scans your workspace dependencies, discovers available extensions, and generates a `.cargo-agents/config.toml` for the project. We recommend checking `.cargo-agents/` into version control so your team shares the same configuration.

## Contents of `.cargo-agents/config.toml`

The project configuration in `.cargo-agents/config.toml` controls which extensions are active for your project. There are three kinds of extensions:

- **Skills** — crate-specific guidance that helps your agent use your dependencies correctly (see [skill definition](./reference/skill-definition.md))
- **Workflows** — tools that improve your agent's development workflow, like running cargo commands more efficiently (see [plugin definition](./reference/plugin-definition.md))
- **MCP servers** — additional capabilities your agent can call on as needed (planned, not yet implemented)

The config file lists each available extension with a simple on/off toggle:

```toml
[skills]
salsa = true
tokio = true

[workflows]
rtk = true
```

When your agent starts, the registered hook runs `cargo agents` in the background. It reads this config and installs the enabled extensions into the locations your agent expects (e.g., `.claude/skills/` for Claude Code). You don't need to do anything, for most agents the hook handles it automatically.

(See the [fine print](./reference/configuration.md) for agent-specific caveats.)

## Synchronizing over time

As your project evolves — dependencies added, removed, or updated — your configuration can get out of date. To bring it back in sync:

```bash
cargo agents sync
```

This does two things:

1. **Updates your project config** (`--workspace`) — re-scans your workspace dependencies and updates `.cargo-agents/config.toml`. New extensions are added, removed dependencies are cleaned up, and your existing on/off choices are preserved.
2. **Updates your agent setup** (`--agent`) — re-installs the enabled extensions into your agent's directories.

You can run either step individually with `cargo agents sync --workspace` or `cargo agents sync --agent`.
