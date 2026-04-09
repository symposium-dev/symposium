# Getting Started

## Install

```bash
cargo binstall cargo-agents       # or `cargo install` if you prefer
```

## Set up your user and project

From your project directory, run:

```bash
cargo agents init
```

This walks you through two things:

1. **User-wide setup** — picks your agent (Claude Code, Copilot, Gemini) and stores it in `~/.symposium/config.toml`. Registers a global hook so your agent picks up project extensions automatically.
2. **Project setup** — scans your workspace dependencies, discovers available extensions, and generates `.symposium/config.toml`.

Check `.symposium/` into version control so your team shares the same configuration. Each developer picks their own agent via `cargo agents init --user`.

You can also run the two steps separately with `cargo agents init --user` and `cargo agents init --project`.

## What's in `.symposium/config.toml`

The project config lists each available extension with a simple on/off toggle:

```toml
[skills]
salsa = true
tokio = true

[workflows]
rtk = true
```

When your agent starts, the registered hook installs the enabled extensions into the locations your agent expects (e.g., `.claude/skills/` for Claude Code). You don't need to do anything — the hook handles it.

## Keeping in sync

As dependencies change, run:

```bash
cargo agents sync
```

This re-scans your dependencies, updates the config, and re-installs extensions. Your existing on/off choices are preserved. See [`cargo agents sync`](./reference/cargo-agents-sync.md) for options.
