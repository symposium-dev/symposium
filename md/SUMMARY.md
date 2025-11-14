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
    - [Rust Crate Sources Tool](./rust-expertise.md)
    - [Rich Collaboration](./rich-collaboration.md)

# Design and implementation

- [Overview](./design/implementation-overview.md)
- [Components](./design/components.md)
- [VSCode Extension](./design/vscode-extension/architecture.md)
    - [Architecture](./design/vscode-extension/architecture.md)
    - [Message Protocol](./design/vscode-extension/message-protocol.md)
    - [State Persistence](./design/vscode-extension/state-persistence.md)
    - [Webview Lifecycle](./design/vscode-extension/webview-lifecycle.md)
    - [Testing](./design/vscode-extension/testing.md)
    - [Testing Implementation](./design/vscode-extension/testing-implementation.md)
    - [Implementation Status](./design/vscode-extension/implementation-status.md)

# References

<!--
    AGENTS: This section is used to store detailed
    research reports that cover specific API details
    you might want.
-->

- [VSCode Webview Lifecycle](./references/vscode-webview-lifecycle.md)
- [Language Server Protocol Overview](./research/lsp-overview/README.md)
    - [Base Protocol](./research/lsp-overview/base-protocol.md)
    - [Language Features](./research/lsp-overview/language-features.md)
    - [Implementation Guide](./research/lsp-overview/implementation-guide.md)
    - [Message Reference](./research/lsp-overview/message-reference.md)
