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
cargo agents init
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

Next, Symposium will ask you whether you want to register hooks **globally** or **per-project**:

* **global** registration means Symposium will activate automatically for all Rust projects.
* **project** registration means Symposium only activates in projects once you run [`cargo agents sync`](./reference/cargo-agents-sync.md) to add hooks to that project.

We recommend **global** registration for maximum convenience.

### Tweaking other settings

You may wish to browse the [configuration](./references/configuration.md) page to learn about other settings, such as how to disable `auto-sync`.

## After setup

Symposium discovers plugins from three sources:

1. **Your workspace** — plugins meant for use when developing crates in your workspace. Skills in `skills/` or `.agents/skills/`, plus any `SYMPOSIUM.toml` files you add. See [Workspace plugins](./workspace.md).
2. **Your dependencies** — workspace deps scanned automatically for crates that contain plugins. See [Auto-discovery](./auto-discovery.md).
3. **Explicit adds** — additional plugin crates you add with `cargo agents use`. See [Plugins via Crates](./installing-plugins.md). These crates can be hosted on crates.io but could also be hosted in a git repo or your local filesystem.

If you maintain a crate and would like to add plugin support to your dependents, see [Supporting your crate](./crate-authors/supporting-your-crate.md).
