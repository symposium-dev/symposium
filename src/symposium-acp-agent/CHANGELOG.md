# Changelog

## [1.3.0](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.2.0...symposium-acp-agent-v1.3.0) - 2026-01-08

### Added

- add cargo distribution type for extensions

### Fixed

- use rustls instead of native-tls for reqwest

### Other

- include claude code and do not block
- Rename CLI commands: act-as-configured -> run, run -> run-with
- Unify CLI to 'run' command and centralize registry access

## [1.2.0](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.1.1...symposium-acp-agent-v1.2.0) - 2026-01-08

### Added

- add configurable agent extensions UI
- add --log-file option to vscodelm_cli example
- add vscodelm_cli example for debugging tool invocation
- accept RUST_LOG-style filter strings in --log argument
- race peek against cancellation in handle_vscode_tool_invocation
- expose one language model per ACP agent
- stream tool calls as markdown in VS Code LM provider
- pass chat request options from VS Code to Rust backend
- introduce HistoryActor for centralized session state management
- implement agent-internal tool permission bridging for vscodelm
- add cancellation support for vscodelm
- support AgentDefinition enum in vscodelm protocol
- add configurable agent backend to vscodelm
- add session UUID logging to vscodelm
- add session actor for VS Code LM provider
- *(vscodelm)* implement Component trait and add tests
- *(vscode)* add Language Model Provider prototype

### Fixed

- strip mcp__ prefix in vscode_tools call_tool handler
- auto-approve tool requests from vscode tools
- don't race against stale cancel_rx when waiting for tool result
- don't race against stale cancel_rx when waiting for tool result
- use actual Eliza response in multi-turn test history
- defer session creation until first request arrives
- use VscodeToolsProxy in Conductor chain for MCP-over-ACP
- wrap agent in Conductor for MCP-over-ACP negotiation
- normalize messages for history matching
- correct McpServer serialization format in TypeScript
- handle all VS Code LM message part types correctly

### Other

- Use spawn_blocking for binary download in resolve_distribution
- VSCode extension uses symposium-acp-agent registry commands
- Add registry subcommands and dynamic agent fetching
- Simplify Ferris component initialization
- Add unit tests for ConfigurationAgent
- Add act-as-configured mode for simplified editor setup
- Consolidate symposium-acp-proxy into symposium-acp-agent
- rename mcp server
- clarify why we are dropping request_state
- more DRY
- move vscodelm tests to separate module
- remove flaky vscodelm integration tests
- add vscodelm integration tests with expect_test assertions
- Revert "fix: use VscodeToolsProxy in Conductor chain for MCP-over-ACP"
- add RequestState::on_cancel helper for racing cancellation
- add cancel_tool_invocation helper and clean up race formatting
- handle_vscode_tool_invocation takes ownership pattern
- replace tokio::select! with futures-concurrency race
- pass invocation_tx to VscodeToolsMcpServer constructor
- apply edits from nikomatsakis review
- refactor session model and unify ContentPart type
- use MatchMessage in process_session_message
- use futures channels and merged streams for vscodelm cancellation
- cleanup logging a bit
- cleanup the method flow
- cleanup the test to avoid mutex
- *(vscodelm)* use expect-test for snapshot testing
- *(vscodelm)* remove unnecessary Arc<Mutex> from Eliza state
- *(vscodelm)* use sacp infrastructure for JSON-RPC

## [1.1.1](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.1.0...symposium-acp-agent-v1.1.1) - 2026-01-01

### Other

- update Cargo.lock dependencies

## [1.1.0](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.0.0...symposium-acp-agent-v1.1.0) - 2025-12-31

### Added

- add built-in ElizACP agent for testing
- add --cargo flag to CLI binaries

### Other

- upgrade elizacp to 11.0.0
- *(vscode)* remove component toggle settings

## [1.0.0](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.0.0-alpha.2...symposium-acp-agent-v1.0.0) - 2025-12-30

### Other

- update Cargo.toml dependencies

## [1.0.0-alpha.2](https://github.com/symposium-dev/symposium/compare/symposium-acp-agent-v1.0.0-alpha.1...symposium-acp-agent-v1.0.0-alpha.2) - 2025-12-30

### Other

- consolidate Ferris MCP server with configurable tools
- upgrade sacp to 10.0.0-alpha.2, sparkle to 0.3.0, rmcp to 0.12

## [0.1.0] - 2025-12-08

- Initial release
