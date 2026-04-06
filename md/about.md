# What is Symposium?

Rust is a great language for agentic development -- but it could be better. On the plus side, Rust's strong type system helps to keep agents on track and catch errors early. And Rust's "efficient by default" design means that your code runs fast and with minimal memory usage.

But on the other hand, unlike some languages that have been around a long time, Rust is dynamic and evolving quickly. Agents are often operating based on stale training data that isn't aware of the latest features available on stable or the "new hotness" when it comes to crates.

And even when a crate is not new, it may have gotchas or surprises that agents and humans alike have to learn to avoid. For humans, those problems tend to be learned once or twice and then ingrained. But agents will happily make the same mistake over and over.

## Leveraging the wisdom of crates.io

Symposium helps to avoid this problem by putting your agent directly in touch with the people who know a crate best: the crate authors themselves. Using Symposium, crate authors can publish skills and other extensions for their crate. Symposium will look through your dependencies and inform your agent about any extra information it may want.

## Keep up with the language developments

Symposium also packages up opinionated language guidance and best practices. Symposium's development team includes core Rust maintainers who make sure that it's kept up-to-date.

## Save tokens and time with our cargo workflow

Symposium also layers on top of the "core cargo" workflow with tools like [rtk](https://github.com/rtk-ai/rtk/), which compresses tokens and time.

## Works however you work

Symposium is meant to fit into any workflow. It can be installed in multiple ways. Some of these methods are more "full featured" than others.

* A Claude Code plugin.
* A skill running with a standalone CLI.
* An MCP server.

See the [install page](./install.md) for details.

## For crate authors

If you maintain a Rust crate, you can publish skills for Symposium so that every AI-assisted user of your library gets your best practices built in. Think of it as documentation that the AI actually reads.

See [Supporting your crate](./crate-authors/supporting-your-crate.md) for how to get started.
