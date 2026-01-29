# What Symposium is

Symposium is a wrapper that extends your AI agent of choice to be a more proficient Rust coder. Symposium is built on the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) which means it is compatible with any AI agent ([Claude Code, Codex, Kiro CLI, etc](./using/symposium.md#selecting-an-agent)) and any editor ([VSCode](./install-vscode.md), [Zed](./install-zed.md), [Rust Rover](./install-rust-rover.md), [NeoVim, Emacs, etc](./install-other.md)).

# Agent mods: a portable extension mechanism

Symposium is based on the idea of [agent mods](./about-mods.md). Mods extend what your agent can do. You might already be familiar with MCP servers or agent skills -- both of these work as mods. But mods can also do things those can't, like intercepting messages or transforming tool output before the agent sees it. And unlike things like Claude Code Plugins, mods work with any ACP-supporting agent.

# Leverage the wisdom of crates.io

Symposium ships with mods for [Rust development](./using/mods.md). But Symposium's superpower is the ecosystem. Anyone can publish mods, and crate authors can recommend mods that help your agent use their libraries well. Just like Rust's crate ecosystem, the community teaches your agent new tricks.

# How do I use it?

Ready to give Symposium a try? It's as easy as [installing an extension in your editor of choice](./install.md):

* [VSCode](./install-vscode.md)
* [Zed](./install-zed.md)
* [Rust Rover](./install-rust-rover.md)
* [NeoVim, Emacs, etc](./install-other.md)
* [...and more!](./install.md)
