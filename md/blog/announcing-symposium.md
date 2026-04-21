# Announcing Symposium - AI The Rust Way

Are you using an AI agent to write Rust code (or curious to try it)? If so, GREAT NEWS! We'd like to share with you **Symposium** - a Rust-focused interoperability layer that connects AI agents to crate-authored skills, tools, and workflows.

(If you've read Niko's [previous][symposium-blog-1] [blog posts][symposium-blog-2] talking about Symposium, this is pretty different! The tool we're announcing today is the result of many iterations of figuring out what exactly the "thing we want" is. So please, read on!)

[symposium-blog-1]: https://smallcultfollowing.com/babysteps/blog/2025/09/24/symposium/
[symposium-blog-2]: https://smallcultfollowing.com/babysteps/blog/2025/10/08/symmacp/

## What is Symposium?

There are realy two answers to that question. The first one is that Symposium is a tool that examines what crates your project depends on and uses that to automatically install new skills, MCP servers, or other extensions. These extensions help your AI agent to write better code, avoid common footguns and pitfalls, and even leverage tools like the [Rust Token Killer (RTK)](GREAT NEWS!) to save you tokens.

The second one is that Symposium is an organization dedicated to one goal, "AI the Rust way", meaning reliable, efficient, and extensible. We are focused on interoperable, vendor-neutral, and community-oriented ways to make agents more reliable and efficient.

## Getting started

You interact with Symposium through the `cargo agents` CLI command. If you want to try it, do this:

```rust
cargo binstall symposium # or `cargo install`
cargo agents init
```

The init command will prompt you to select what agents you want to use and a few other things. Based on that we install hooks that will cause Symposium to be invoked automatically. The next time you start an agent on a Rust project, Symposium will check if there are available skills or other extensions for the crates you use and set them up automatically. You shouldn't have to do anything else.

# Symposium helps your agent write better code and use fewer tokens

You may be familiar with various extensions that agents can work with, such has [MCP servers](https://modelcontextprotocol.io/docs/getting-started/intro), [Skills](https://agentskills.io/home), or Hooks. You may also know that different agents have different levels of support for these, and even different takes on them (Hooks, for example, are not as well-standardized as MCP servers and Skills). However, that doesn't diminish the fact that many people have built many tools around these extension systems. We want you to *easily* use these ecosystem tools.

You may *also* have run into cases where a model is "outdated" compared to either the Rust language itself (e.g., there may be a newer language feature that is more idiomatic) or was trained on an older versions of a crate that you are using. It's generally not hard to get models to follow newer conventions, but they need to be *told* to do so. We want to make that easier and more automated. 

Finally, we want writing code with agents to be more *efficient* and *reliable*. Some of this comes from the above two goals, but part of it also comes from making sure that agents write code the way *you* would write it. For example, when you finish writing Rust code, you likely run `cargo check`, run your tests, or format your code - and we think that you should expect your agent to do the same. Simulatenously, efficiency *also* means that we want these tools to use as few tokens as possible.

# Symposium Plugins

A Symposium **plugin** defines a set of extensions (mcp servers, skills, hooks, etc) and the conditions in which they should be used (currently: when a given version of a given crate is in the project's dependencies). Plugins are hosted on repositories called a "plugin source"; we define a [central repository](https://github.com/symposium-dev/recommendations) with our globally recommended plugins, but you can additional plugin sources of your own if you like.

## Skills

[Agent Skills](https://agentskills.io/home) are a lightweight format for defining specialized knowledge or workflows for agents to use. Most agents have a pre-defined list of places that they look for skills, but don't *currently* have a way to dynamically make them available.

In Symposium, we automatically discover skills from plugins applicable to the current crate. By default, we automatically sync them to the current project's directory so they can be used by your agent (either `.agent/skills` or `.claude/skills`). This is done through a custom hook (if your agent supports it), but can be disabled or manually synced with `cargo agents sync`.

## Hooks

Unlike skills which are dynamically loaded by agents, hooks are dispatched on certain events such as on agent start, after a user prompt, or prior to a tool use. Symposium has a small number of hooks it installs (when available) that it uses to ensure that plugins are discovered and loaded for an agent to use.

Additionally, today, hooks defined by plugins are also dispatched through Symposium. This allows, for example, dispatching hooks written for one agent when using a different agent (to the extent that we've implemented support). The list of supported hooks is fairly small, but we're far from done with expanding hooks support.

## MCP servers

MCP servers were one of the first extensions made available by agents. They expose a set of tools, either local or remote, that agents can call. MCP servers defined by a Symposium plugin get installed into your agent's settings for use.

# What's next?

As we said in the beginning, the Symposium org is focused on "AI the Rust way" -- so what does that mean? We're starting with a minimal, usual product for users to experiment with and hopefully find use from. But, we're far from done. We have a number of really interesting ideas to make Symposium even more useful.

We want to continue to expand the set of agent features that Symposium supports. When an agent supports a tool or similar, we want it to be a minimal process to be able to recommend that users of your crate also use that tool. Often, this means that we should "just install" those tools into project-local agent settings; but, we want to make sure that this is done *correctly* and supports the agents that our users use. However, we *also* want to support (when possible) more dynamic loading - such as by dispatching hooks through Symposium itself, or having Symposium register a transparent MCP server layer. There are lots of things we can do here, and we're excited to hear what people want and need first.

We currently have pretty minimal support for how to run hooks or MCP servers - really just a command to run. We already have in-progress work to support declarative dependencies, which in turns allows both auto-installation and auto-updates. Using a symposium plugin should "just work".

The work we're presenting today is focused mainly around Rust *crates*, but our vision also includes better recommendations around the Rust *language*. We've already seen [a](https://github.com/leonardomso/rust-skills) [few](https://github.com/actionbook/rust-skills) ecosystem-driven projects with this goal - we plan to review these and find what works best for Symposium users and make it the default for the best experience possible when writing Rust code. Similarly, we plan to write our own plugins that help your agent format and test Rust code that it has written, before you even look at it.

Symposium previously was focused around the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/get-started/introduction), which provides a programmatic way to interact with and extend agent capabilities. We still love this vision, but our current focus is on an ecosystem-first approach of meeting agents where they are today. We do expect that as ACP adoption continues to increase and we have a solid foundation with the work we've presented today, that we will again focus on ACP to further increase the interoperability and extensibility we provide for users of Symposium.

Finally, although our initial work is focused around Rust, we think this idea - discoverability and use of plugins defined by dependencies - is applicable and useful for other language ecosystems too. We would love to expand this to other languages.

In all, we're really excited for people to use Symposium. We hope that what we've shared today gets you excited about building better Rust with AI, and we think that this is only the beginning. If you have thoughts or questions, either [open an issue](https://github.com/symposium-dev/symposium/issues) on Github or join the [Symposium Zulip](https://symposium-dev.zulipchat.com); we'd love to hear your thoughts!
