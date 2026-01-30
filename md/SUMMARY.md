<!--
    STYLE GUIDELINES:

    - Avoid promotional language: no "rich", "powerful", "easy", etc.
    - No "Benefits" sections - they're promotional by nature
    - No duplicate table of contents at chapter ends
    - Be factual and technical, not persuasive
    - Describe what the system does, not why it's good
-->

# Summary

- [Introduction](./introduction.md)
- [About](./about.md)
  - [Agent mods](./about-mods.md)

# Using Symposium

- [How to install](./install.md)
    - [VSCode](./install-vscode.md)
    - [Zed](./install-zed.md)
    - [Rust Rover](./install-rust-rover.md)
    - [Other editors](./install-other.md)
- [Using Symposium](./using/symposium.md)
- [Configuration](./using/configuration.md)
- [Built-in mods](./using/mods.md)
    - [Sparkle](./using/sparkle.md)
    - [Ferris](./using/ferris.md)
    - [Cargo](./using/cargo.md)

# Authoring Agent Mods

- [Creating Agent Mods](./mods/creating-mods.md)
- [Recommending Agent Mods](./mods/recommending-mods.md)
- [Publishing Agent Mods](./mods/publishing-mods.md)

# Contributing to Symposium

- [How to contribute](./contribute.md)
  - [Overview](./design/implementation-overview.md)
  - [Common Issues](./design/common-issues.md)
  - [Distribution](./design/distribution.md)
  - [Agent Registry](./design/agent-registry.md)
  - [Agent Mods](./design/mods.md)
  - [Components](./design/components.md)
  - [Run Mode](./design/run-mode.md)
  - [Rust Crate Sources](./design/rust-crate-sources.md)
  - [VSCode Extension](./design/vscode-extension/architecture.md)
      - [Message Protocol](./design/vscode-extension/message-protocol.md)
      - [Tool Authorization](./design/vscode-extension/tool-authorization.md)
      - [State Persistence](./design/vscode-extension/state-persistence.md)
      - [Webview Lifecycle](./design/vscode-extension/webview-lifecycle.md)
      - [Testing](./design/vscode-extension/testing.md)
      - [Testing Implementation](./design/vscode-extension/testing-implementation.md)
      - [Packaging](./design/vscode-extension/packaging.md)
      - [Mods UI](./design/vscode-extension/mods.md)
      - [Language Model Provider](./design/vscode-extension/lm-provider.md)
      - [Language Model Tool Bridging](./design/vscode-extension/lm-tool-bridging.md)
      - [Implementation Status](./design/vscode-extension/implementation-status.md)
  - [Reference material](./references/index.md)
      - [MynahUI GUI Capabilities](./references/mynah-ui-guide.md)
      - [VSCode Webview Lifecycle](./references/vscode-webview-lifecycle.md)
      - [VSCode Language Model Tool API](./references/vscode-lm-tool-api.md)
      - [VSCode Language Model Tool Rejection](./references/vscode-lm-tool-rejection.md)
      - [GitHub Actions Rust Releases](./references/gh-actions-rust-releases.md)
      - [Language Server Protocol Overview](./research/lsp-overview/README.md)
          - [Base Protocol](./research/lsp-overview/base-protocol.md)
          - [Language Features](./research/lsp-overview/language-features.md)
          - [Implementation Guide](./research/lsp-overview/implementation-guide.md)
          - [Message Reference](./research/lsp-overview/message-reference.md)
