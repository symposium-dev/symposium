# What is Symposium?

Symposium is a one-stop shop to help agents write great Rust code. It connects you to best-in-class tools and workflows but it also serves up skills and extensions tailored to the crates you are using, all authored by the people who know those crates best -- the crate authors.

## `init` and go

Getting started with Symposium is easy:

```rust
cargo binstall symposium       # or `cargo install` if you prefer
cargo agents init
```

The [`init` command](./references/cargo-agents-init.md) will guide you through picking your personal agent. It will then configure the agent to use Symposium (e.g., by installing hooks). This will immediately give you some benefits, such as introducing Rust guidance and reducing token usage with the [`rtk`](https://www.rtk-ai.app/) project.

## Leveraging the wisdom of crates.io

To truly get the most out of Symposium, you also want to install it into your project. When you run `cargo agents init` in a project directory, it will scan your dependencies and create customized skills, tools, and other improvements. These extensions are source either from our central [recommendations repository](https://github.com/symposium-dev/recommendations). In the future, we plan to enable crate authors to embed extensions within their crates themselves and skip the central repo altogether.

## Everybody picks their own agent

Work on an open-source project or a team where people use different agents? No problem. Your Symposium configuration is agent agnostic, and the `cargo agents` tool adapts it to the agent that each person is using. You can also specify the agent centrally if you prefer.

## Staying synchronized

By default, Symposium is setup to synchronize itself. It'll source the latest skills automatically and add them to your project. If you prefer, you can disable auto-updates and run `cargo agents sync` manually.

## For crate authors

If you maintain a Rust crate, you can publish skills for Symposium so that every AI-assisted user of your library gets your best practices built in. See [Supporting your crate](./crate-authors/supporting-your-crate.md) for how to get started.
