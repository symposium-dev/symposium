# A Maturing Symposium

**Authored By: Jack Huey**

We announced an initial MVP release of Symposium just about two months ago, and figured it would be good to give an update on what we've added since!

Tl;dr we've added a number of different features that allow Symposium to do more things for different workflows, and to keep your plugins updated!

## More powerful plugins

The first set of changes to discuss all cover essentially the *things* that you can do with plugins.

**Unified predicate system.** In the MVP, a plugin only had a single `crates` field to filter when it was activated.
We've added a more robust and more general `predicates` field that supports more complex logic for plugin (and e.g. skills) activation. We currently support builtin `crate`, `shell`, `path_exists`, and `env` predicates; as well as `not`/`any`/`all` combinators. So, you can, for example gate the activation of a plugin based on if a program is installed, or if a workspace env var is set. The existing `crates` field desugars into `any(crate(..), ..)`. Additionally, plugins can register *custom predicates*, available globally across plugins. This is particularly useful to gate plugin use on if something like [battery packs](https://github.com/battery-pack-rs/battery-pack) is enabled or if `async` is used.

**Plugin-vended subcommands.** Skills, hooks, and MCP servers cover a lot, but sometimes a plugin just wants to offer a command that you, or your agent, can run directly. Now plugins can contribute their own `cargo agents <name>` subcommands, and they only appear when the surrounding project makes them relevant.

**Hooks 2.0.** One thing we really want is the ability to *just use* the ecosystem - so if a Claude Code hook exists, you should just be able to use that in a plugin. But, we also want to support the ability to "write once, run anywhere". We've added support for a common hook infrastructure, and hook dispatch will fall back to that if a native hook doesn't exist. We eventually want to potentially extend that to *custom hook events* too, to enable something like "post cargo test run".

Of course, the schema has changed a couple times since the initial MVP. But, we try to validate plugin definitions to ensure that what you wrote makes sense.

## More control of your plugin sources and where they are installed

In addition to making your plugins *more powerful*, we also have given you more control of where your plugins are sourced and where they are installed.

**Global install + env vars.** By default, plugin dependencies (installations) are installed into the `~/.symposium` directory. However, we've added the ability to install globally. Additionally, we've added env vars that are set to the installation paths and binaries, for more control.

**Crate-sourced skills.** Crates can now ship agent skills directly. Skills can use `source = "crate"`. You can use either the *current* crate, or a separate crate. Additionally, you can decide what skills are active when *developing the current crate*, and which skills are active when *using the current crate*. Skills written to `./skills` are used for crate *use*; skills written to `.agents/skills` are for crate *development*. When you write a skill to `.agents/skills` , they will be synced to `.claude/skills` or other agent skills directories, if needed. Automatically synced skills are git-ignored (as are all symposium-installed skills), so you don't need to worry about your git workspace getting cluttered!

## Keeping Symposium and your plugins up to date

We all want our software to be up-to-date. Similarly, when a crate author publishes a newer version of a skill, we want to ensure that the latest version is used.

**Plugin and Symposium Auto-Update.** We've added a `auto-update` config option (on by default) that will automatically update Symposium. This can be manually triggered with `cargo agents self-update`. Plugin hooks and dependencies now also automatically update; skills already updated automatically.

**Smarter, cheaper sync.** Auto-sync of skills now skips when `Cargo.lock` is unchanged, has a configurable debounce (`sync-debounce-secs`, default 5s), is change-aware (no disk churn when nothing differs), and dedups/disambiguates installed skill dirs by *origin* rather than name.

## Helping agents (and humans) use Symposium better

Although we want Symposium to be mostly "hands off" once set up, we recognize that sometimes we (humans *and* agents) need to interact with it. We've added a couple things to help with this.

**Audience-grouped `--help`.** `cargo agents --help` now renders two sections: "Commands for humans" and "Commands for agents". It also lists workspace-applicable plugin subcommands alongside built-ins. SessionStart also nudges the agent to run `--help` when relevant subcommands exist.

**Structured output (`--verbose` / `--json`).** A new report layer renders command output three ways: human-readable (default), verbose decision trace to stderr, or a machine-readable JSON array. This is intended in large part for debugging or machine/tooling consumption.

## Upcoming opt-in telemetry to help us ensure Symposium is effective

We've added the basic infrastructure to support anonymous, local, opt-in telemetry. Nothing is collected, yet. But, we want to be able to eventually let users help us make Symposium better: we want to know what's working and what isn't. Telemetry status and information can be checked with `cargo agents telemetry status` and `cargo agents telemetry show`.

## Conclusions

In general, we're proud of the progress we've made. But, we still have more that we want to do! We're looking into supporting additional language ecosystems (e.g. npm or pypi) and we want to be flexible to new standards as they are developed. We want Symposium to empower you to do things you otherwise couldn't or wouldn't due to complexity or cognitive overhead, while also meeting the ecosystem where it is.
