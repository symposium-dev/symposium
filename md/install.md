# Getting Started

## Install

```bash
cargo binstall symposium       # or `cargo install` if you prefer
```

## Set up

```bash
symposium init
```

This will prompt you to select the agents you use (Claude Code, Copilot, Gemini, etc.) — you can pick more than one. Once you've done that, it installs hooks and plugins for your selected agents and you're good to go.

Symposium will now install skills based on your dependencies automatically.

If you'd prefer to sync skills manually or to tweak the plugins that symposium uses, you can! See the [configuration reference](./reference/configuration.md) for details.
