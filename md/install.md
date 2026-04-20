# Getting Started

## Install

The fastest way to install Symposium is with `cargo binstall`:

```bash
cargo binstall symposium
```

If you prefer to build from source, use `cargo install` instead:

```bash
cargo install symposium
```

## Initialization

Once you have installed Symposium, you need to run the [`init` command](./references/cargo-agents-init.md):

```bash
symposium init
```

### Select your agents

This will prompt you to select the agents you use (Claude Code, Copilot, Gemini, etc.) — you can pick more than one:

```bash
Which agents do you use? (space to select, enter to confirm):
> [ ] Claude Code
  [x] Codex CLI
  [ ] GitHub Copilot
  [ ] Gemini CLI
  [ ] Goose
  [x] Kiro
  [x] OpenCode
```

### Global vs project hook registration

Next, Symposium will ask you whether you want to register hooks **globally** or **per-proejct**:

* **global** registration means Symposium will activate automatically for all Rust projects.
* **project** registration means Symposium only actives in projects once you run [`symposium sync`](./reference/cargo-agents-sync.md) to add hooks to that project.

We recommend **global** registration for maximum convenience.

### Tweaking other settings

You may wish to browse the [configuration](./references/configuration.md) page to learn about other settings, such as how to disable `auto-sync`.

## After setup

Symposium will now install skills, MCP servers, and other extensions based on your dependencies automatically.

Currently all the plugins installed by Symposium can be found in the [central recommendations repository][rr]. We expect eventually to allow crates to define their own plugins without any central repository, but not yet. If you have a crate and would like to add a plugin for it to symposium, see the [Supporting your crate](./crate-authors/supporting-your-crate.md) page.

[rr]: https://github.com/symposium-dev/recommendations

If you have private crates or would like to install plugins for your own use, you can consider adding a [custom plugin source](./custom-plugin-source.md).
