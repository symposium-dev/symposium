# Using Symposium

Symposium focuses on creating the best environment for Rust coding through **Agent Mods** - MCP servers that add specialized tools and context to your agent.

Symposium is built on the Agent Client Protocol (ACP), which means the core functionality is portable across editors and environments. VSCode is the showcase environment with experimental GUI support, but the basic functionality can be configured in any ACP-supporting editor.

The instructions below use the VSCode extension as the basis for explanation.

## Selecting an Agent

To select an agent, click on it in the agent picker. Symposium will download and install the agent binary automatically.

Some agents may require additional tools to be available on your system:
- **npx** - for agents distributed via npm
- **uvx** - for agents distributed via Python
- **cargo** - for agents distributed via crates.io (uses `cargo binstall` if available, falls back to `cargo install`)

Symposium checks for updates and installs new versions automatically as they become available.

For adding custom agents not in the registry, see [VSCode Installation - Custom Agents](../install-vscode.md#custom-agents).

## Managing Mods

Mods add capabilities to your agent. To configure mods, type `/symposium:config` in the chat.

The configuration menu lets you:
- **Toggle** mods on/off by typing their number
- **Reorder** mods with `move X to Y`
- **Save** changes with `SAVE` or discard with `CANCEL`

**Order matters** - mods are applied in the order listed. The first mod is closest to the editor, and the last is closest to the agent.

On first run, Symposium recommends mods based on your workspace (e.g., Cargo mod for Rust projects). You can adjust these recommendations in the config menu.

## Adding Your Own Recommendations

You can add recommendations for mods that Symposium should suggest to you:

- **For all your workspaces**: Create a `recommendations.toml` file in your [config directory](./configuration.md#configuration-location)
- **For a specific project**: Create a `.symposium/recommendations.toml` file in the project root

This is useful for internal or proprietary mods, or mods that aren't in the central recommendations yet. See [Recommending Mods](../mods/recommending-mods.md) for the file format.

## Builtin Mods

Symposium ships with three builtin mods:

- **[Sparkle](./sparkle.md)** - AI collaboration framework that learns your working patterns
- **[Ferris](./ferris.md)** - Rust crate source inspection
- **[Cargo](./cargo.md)** - Compressed cargo command output
