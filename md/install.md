# Installing Symposium

Symposium is meant to fit into any workflow. It can be installed in multiple ways. 

## Capabilities by method

Symposium offers several advantages for Rust development but not all of them are supposed by all installation methods.

| Method | Crate-specific guidance | Rust-language updates | Improved token usage, workflow |
| --- | --- | --- | --- |
| Claude Code Plugin | ✅ | ✅ | ✅ Always | 
| Skill | ✅ | ✅ | 🟡 Best effort | 
| MCP Server | ✅ | ✅ | 🟡 Best effort | 

* *Crate-specific guidance* -- make your agent aware of skills for the crates you are using, help your agent find the source for a crate
* *Rust-language updates* -- provide your agent with info on the latest language changes, guidance on how best to use Rust
* *Improved token usage, workflow* -- filter output of cargo and invoke other tools like rustfmt automatically
  * When using a plugin, we intercept all calls to bash to do this determinstically
  * When using a skill or MCP server, we advice your agent to use our tools but can't guarantee it

## Claude Code Plugin

To install as a Claude Code plugin, run these two steps. First add the symposium "marketplace" from github:

```bash
claude plugin marketplace add symposium-dev/symposium-claude-code-plugin
```

Then install the symposium plugin from that marketplace:

```bash
claude plugin install symposium@symposium
```

This plugin will install hooks and a `/symposium:rust` skill. The skill *should* be automatically invoked when you start with Rust code, but you can always invoke it manually if you like. Both the hooks and the plugin will download the Symposium binary appropriate for your architecture from our Github releases automatically, presuming it's not otherwise found on your system.

## Skill

If you prefer, you can install the Symposium Skill independently from the plugin. It is found in this directory:

https://github.com/symposium-dev/symposium-claude-code-plugin/tree/main/skills/rust

The skill *should* be automatically invoked when you start with Rust code, but you can always invoke it manually if you like. The skill will download the Symposium binary appropriate for your architecture from our Github releases automatically, presuming it's not otherwise found on your system.

## MCP server

You can also install Symposium as an MCP server. To do that, you'll need to [install the CLI tool](#installing-the-symposium-cli-tool). You can then configure it as follows

```json
{
    "symposium": {
        "command": "symposium",
        "args": ["mcp"]
    }
}
```

## Installing the `symposium` CLI tool

Manually installing the symposium CLI tool [requires installing the Rust toolchain](https://rustup.rs/). Given that Symposium targets Rust development, this is hopefully not a problem for you!

### Installing from crates.io

You can install the latest release of symposium directly from `crates.io`:

* `cargo binstall symposium` -- download prebuilt binary, faster, requires [cargo binstall](https://github.com/cargo-bins/cargo-binstall)
* `cargo install symposium` -- build from source

### Installing from the git repository

* Check out the [symposium-dev/symposium](https://github.com/symposium-dev/symposium) repository
* Run `cargo install --path .` from inside the checkout
