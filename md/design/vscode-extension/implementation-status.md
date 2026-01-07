# Implementation Status

This chapter tracks what's been implemented, what's in progress, and what's planned for the VSCode extension.

## Core Architecture

- [x] Three-layer architecture (webview/extension/agent)
- [x] Message routing with UUID-based identification
- [x] HomerActor mock agent with session support
- [x] Webview state persistence with session ID checking
- [x] Message buffering when webview is hidden
- [x] Message deduplication via last-seen-index tracking

## Error Handling

- [x] Agent crash detection (partially implemented - detection works, UI error display incomplete)
- [ ] Complete error recovery UX (restart agent button, error notifications)
- [ ] Agent health monitoring and automatic restart

## Agent Lifecycle

- [x] Agent spawn on extension activation (partially implemented - spawn/restart works, graceful shutdown incomplete)
- [ ] Graceful agent shutdown on extension deactivation
- [ ] Agent process supervision and restart on crash

## ACP Protocol Support

### Connection & Lifecycle

- [x] Client-side connection (`ClientSideConnection`)
- [x] Protocol initialization and capability negotiation
- [x] Session creation (`newSession`)
- [x] Prompt sending (`prompt`)
- [x] Streaming response handling (`sessionUpdate`)
- [ ] Session cancellation (`session/cancel`)
- [ ] Session mode switching (`session/set_mode`)
- [ ] Model selection (`session/set_model`)
- [ ] Authentication flow

### Tool Permissions

- [x] Permission request callback (`requestPermission`)
- [x] MynahUI approval cards with approve/deny/bypass options
- [x] Per-agent bypass permissions in settings
- [x] Settings UI for managing bypass permissions
- [x] Automatic approval when bypass enabled

### Session Updates

The client receives `sessionUpdate` notifications from the agent. Current support:

- [x] `agent_message_chunk` - Display streaming text in chat UI
- [x] `tool_call` - Logged to console (not displayed in UI)
- [x] `tool_call_update` - Logged to console (not displayed in UI)
- [ ] Execution plans - Not implemented
- [ ] Thinking steps - Not implemented
- [ ] Custom update types - Not implemented

**Gap:** Tool calls are logged but not visually displayed. Users don't see which tools are being executed or their progress.

### File System Capabilities

- [ ] `readTextFile` - Stub implemented (throws "not yet implemented")
- [ ] `writeTextFile` - Stub implemented (throws "not yet implemented")

**Current state:** We advertise `fs.readTextFile: false` and `fs.writeTextFile: false` in capabilities, so agents know we don't support file operations.

**Why not implemented:** Requires VSCode workspace API integration and security considerations (which files can be accessed, path validation, etc.).

### Terminal Capabilities

- [ ] `createTerminal` - Not implemented
- [ ] Terminal output streaming - Not implemented
- [ ] Terminal lifecycle (kill, release) - Not implemented

**Why not implemented:** Requires integrating with VSCode's terminal API and managing terminal lifecycle. Also involves security considerations around command execution.

### Extension Points

- [ ] Extension methods (`extMethod`) - Not implemented
- [ ] Extension notifications (`extNotification`) - Not implemented

These allow protocol extensions beyond the ACP specification. Not currently needed but could be useful for custom features.

## State Management

- [x] Webview state persistence within session
- [x] Chat history persistence across hide/show cycles
- [ ] Draft text persistence (FIXME: partially typed prompts are lost on hide/show)
- [ ] Session restoration after VSCode restart
- [ ] Workspace-specific state persistence
- [ ] Tab history and conversation export

## Language Model Provider (Experimental)

> Set `symposium.enableExperimentalLM: true` in VS Code settings to enable.

This feature exposes ACP agents via VS Code's `LanguageModelChatProvider` API, allowing them to appear in the model picker for use by Copilot and other extensions.

**Status:** Experimental, disabled by default. May not be the right approach.

- [x] TypeScript: LanguageModelChatProvider registration
- [x] TypeScript: JSON-RPC client over stdio
- [x] TypeScript: Progress callback integration
- [x] Rust: `vscodelm` subcommand
- [x] Rust: Session actor with history management
- [x] Rust: Tool bridging (symposium-agent-action for permissions)
- [x] Rust: VS Code tools via synthetic MCP server
- [x] Feature flag gating (`symposium.enableExperimentalLM`)
- [ ] Fix: Multiple MCP tools cause invocation failures

**Known issue:** Tool invocation works with a single isolated tool but fails when multiple VS Code-provided tools are bridged. Root cause unknown.

**Open question:** VS Code LM consumers inject their own context (project details, editor state, etc.) into requests. ACP agents like Claude Code also inject context. These competing context layers may confuse the model, making the LM API better suited for raw model access than wrapping full agents.

See [Language Model Provider](./lm-provider.md) and [Tool Bridging](./lm-tool-bridging.md) for architecture details.
