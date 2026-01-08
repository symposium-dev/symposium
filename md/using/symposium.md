# Using Symposium

Symposium focuses on creating the best environment for Rust coding through **Agent Extensions** - MCP servers that add specialized tools and context to your agent.

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

## Managing Extensions

Extensions add capabilities to your agent. Open the **Settings** panel to manage them.

In the Extensions section you can:
- **Enable/disable** extensions via the checkbox
- **Reorder** extensions by dragging the handle
- **Add** extensions via the "+ Add extension" link
- **Delete** extensions from the list

**Order matters** - extensions are applied in the order listed. The first extension is closest to the editor, and the last is closest to the agent.

When adding extensions, you can choose from:
- **Built-in** extensions (Sparkle, Ferris, Cargo)
- **Registry** extensions from the shared catalog
- **Custom** extensions via executable, npx, pipx, cargo, or URL

## Builtin Extensions

Symposium ships with three builtin extensions:

- **[Sparkle](./sparkle.md)** - AI collaboration framework that learns your working patterns
- **[Ferris](./ferris.md)** - Rust crate source inspection
- **[Cargo](./cargo.md)** - Compressed cargo command output
