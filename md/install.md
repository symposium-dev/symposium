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

Once you have installed Symposium, you need to run the [`init` command](./reference/cargo-agents-init.md):

```bash
cargo agents init
```

### Select your agents

This will prompt you to select the agents you use (Antigravity, Claude Code, Copilot, etc.) — you can pick more than one:

```bash
Which agents do you use? (space to select, enter to confirm):
> [ ] Antigravity CLI
  [ ] Claude Code
  [x] Codex CLI
  [ ] GitHub Copilot
  [ ] Goose
  [x] Kiro
  [x] OpenCode
```

### Global vs project hook registration

Next, Symposium will ask you whether you want to register hooks **globally** or **per-project**:

* **global** registration means Symposium will activate automatically for all Rust projects.
* **project** registration means Symposium only activates in projects once you run [`cargo agents sync`](./reference/cargo-agents-sync.md) to add hooks to that project.

We recommend **global** registration for maximum convenience.

### Tweaking other settings

You may wish to browse the [configuration](./reference/configuration.md) page to learn about other settings, such as how to disable `auto-sync`.

## After setup

Symposium will now install skills, MCP servers, and other extensions based on your dependencies automatically.

Most plugins come from two places: the [central recommendations repository][rr], which is enabled by default, and crates your project depends on directly that ship their own. A dependency's plugin stays off until you enable it: an interactive [`cargo agents sync`](./reference/cargo-agents-sync.md#consent-prompt) asks about each one, and [`cargo agents status`](./reference/cargo-agents-status.md) lists the ones waiting for an answer. If you have a crate and would like to add a plugin for it to symposium, see the [Supporting your crate](./crate-authors/supporting-your-crate.md) page.

[rr]: https://github.com/symposium-dev/recommendations

If you have private crates or would like to install plugins for your own use, you can consider adding a [custom plugin source](./custom-plugin-source.md).
