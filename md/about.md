# What is Symposium?

Symposium is a tool to help you get the most from your agents. It connects you to best-in-class tools and workflows but it also serves up skills and extensions tailored to the crates you are using, all authored by the people who know those crates best -- the crate authors.

## `init` and go

Getting started with Symposium is easy:

```bash
cargo install symposium       # or `cargo binstall` if you prefer
cargo agents init
```

The [`init` command](./references/cargo-agents-init.md) will guide you through picking your personal agent. It will then configure the agent to use Symposium (e.g., by installing hooks). This will also install a curated set of plugins, such as idiomatic Rust guidance to help your agent avoid common pitfalls.

## Leveraging the wisdom of crates.io

You can use the `cargo agents` command to install agent plugins from crates.io.

*Example:* The [dial9](https://github.com/dial9-rs/dial9) project provides telemetry that can help you optimize your service. It also contains skills to help your agent query the data and diagnose performance problems for you. You can install those skills by doing:

```bash
cargo agents install dial9
```

## Automatically discovering plugins from your dependencies

When you use an agent in a project, Symposium will scan your dependencies and automatically install any plugins embedded in them (but only within the context of that project). This lets crates provide skills, MCP servers, hooks, or other extensions that help your agent know how to use the crate.

*Example*: The [`assert-struct`](https://crates.io/crates/assert-struct) crate is a nice macro to let you write concise assertions. When you have it as a dependency in your project, Symposium will automatically install embedded skills that teach your agent when/how to use it best.

## Installing plugins within your workspace

Sometimes you have skills or plugins that are useful when *developing* your crate but are not useful to people that *depend* on your crate. No problem. If your project has skills in `.agents/skills`, Symposium will pick them up and install them into the appropriate directory for the tools your developers are using (some tools use `.agents/skills`, but others use e.g. `.claude/skills`). You can also add MCP servers or hooks gated by the [`workspace()` predicate](./reference/predicates.md) so that they only apply within the crate's workspace.

## Agent agnostic: let everybody pick their own agent

Everything in Symposium is designed on the premise that extensions ought to work uniformly. When you write a skill, an MCP server, or a hook, you want it to work with any agent that the user happens to be using. We automatically bridge gaps where we can.

## Agent specialized: beyond the least common denominator

On the other hand, the reality is that different agents have different capabilities. When you want to, we let you tailor your advice to specific agents, specific models, or specific versions. That means you help your users get the most of specialized capabilities while having fallback capabilities when people need them.

## For crate authors

If you maintain a Rust crate, you can publish skills for Symposium so that every AI-assisted user of your library gets your best practices built in. Just add a `SYMPOSIUM.toml` to your crate and then add your crate to the allow list on our central repository (this is a temporary security step). If you don't own the crate and the owner does not want to include the skills in their crate, you can still upload skills for it into our central repository. See [Supporting your crate](./crate-authors/supporting-your-crate.md) for how to get started.
