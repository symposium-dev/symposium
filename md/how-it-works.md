# How it works

Symposium is built on [ACP](https://github.com/anthropics/agent-client-protocol)'s proxy architecture. A proxy sits between your editor and your agent, observing and augmenting the conversation. This position lets it do things MCP servers can't:

- Intercept and modify agent behavior
- Inject context based on your project's dependencies
- Coordinate multi-step workflows
- Contribute UI components to the editor

## Current components

### Ferris

[Ferris](https://github.com/symposium-dev/ferris) is an MCP server that gives agents access to Rust-specific patterns and information.

### symposium-cargo

[symposium-cargo](https://github.com/symposium-dev/symposium-cargo) wraps the `cargo` command, capturing build output and providing it to your agent in a structured format. This gives agents better insight into compilation errors, warnings, and test results.

### Sparkle

[Sparkle](https://symposium-dev.github.io/sparkle/) helps agents develop collaborative working patterns. It helps agents remember how you like to work, maintain context across sessions, and engage more naturally in back-and-forth problem solving.

## What we're experimenting with

- **IDE operations**: Bringing language server capabilities (go-to-definition, find references, refactoring) directly to agents
- **Error explanations**: Rich, context-aware explanations of Rust compiler errors
- **Crate-provided tooling**: Allowing crate authors to ship agent capabilities alongside their libraries
- **Walkthroughs**: Interactive guided explorations of code and concepts
- **Taskspaces**: Isolated working contexts for complex multi-step tasks
