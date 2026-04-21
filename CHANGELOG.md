# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/symposium-dev/symposium/compare/symposium-v0.1.0...symposium-v0.2.0) - 2026-04-21

### Added

- add Gemini BeforeAgent hook and cross-agent event mapping
- filter to direct dependencies only
- crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- directory-based plugins with SYMPOSIUM.toml and plugin source discovery
- dual-mode agent integration test infrastructure
- add hook-scope config (global vs project)
- wire plugin MCP servers through sync_agent
- add MCP server registration and unregistration for all agents
- add McpServerEntry type and mcp_servers field to plugin manifest
- add plugin format routing via HookFormat
- add support for Codex CLI, Kiro, OpenCode, and Goose agents
- add --remove-agent flag to init and sync
- support multiple agents via [[agent]] config entries
- support project-level plugin sources and self-contained mode
- add SessionStart hook with plugin session-start-context
- reframe install, about page
- implement cargo-agents CLI with init, sync, and hook flows

### Fixed

- fixup! WIP--merge into 1: refactor: restructure documentation navigation and consolidate pages
- fixup! docs: expand install guide and agent MCP server documentation
- fixup! fix: auto-sync cwd resolution falls back to process cwd
- fixup! feat: crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- fixup! feat: directory-based plugins with SYMPOSIUM.toml and plugin source discovery
- fixup! feat: directory-based plugins with SYMPOSIUM.toml and plugin source discovery
- fixup! feat: directory-based plugins with SYMPOSIUM.toml and plugin source discovery
- fixup! feat: crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- fixup! feat: crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- fixup! feat: crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- auto-sync cwd resolution falls back to process cwd
- use XDG_STATE_HOME for logs directory
- Kiro agent config needs tools and resources fields
- handle events without agent-specific handlers gracefully
- make --add-agent additive and fix project agent resolution
- pre-select existing agents in prompt and remove hooks on unconfig
- split Copilot hook registration for global vs project paths
- use Symposium.home_dir for global hook registration instead of dirs::home_dir

### Other

- update the anem of the book
- fmt
- Support cargo subcommand convention for 'cargo agents'
- fmt
- Keep 'symposium' as the agent/server identity name
- Rename binary from symposium to cargo-agents
- hide agent-facing CLI commands and rename `crate` to `crate-info`
- fixup warning
- cargo fmt
- WIP--merge into 1: refactor: restructure documentation navigation and consolidate pages
- WIP--merge into 6: feat: crate predicate system with wildcard, plugin-level filtering, and MCP server filtering
- WIP--merge into 5: feat: directory-based plugins with SYMPOSIUM.toml and plugin source discovery
- WIP--merge into 1: refactor: restructure documentation navigation and consolidate pages
- remove session_start_context from plugins
- remove activation field from skills
- expand install guide and agent MCP server documentation
- restructure documentation navigation and consolidate pages
- pacify the merciless cargo fmt
- simplify configuration: remove per-project config
- Simplify CLI: remove start command, crate --list, and skills output
- Remove built-in MCP server
- Remove start command from MCP server, inline dispatch into CLI
- Merge pull request #175 from nikomatsakis/azure-range
- pacify the merciless cargo fmt
- Add test for non-object container recovery in JSON MCP registration
- Fix Goose stale-entry updates and add YAML quoting tests
- Clarify Copilot MCP registration format in design doc
- Strengthen Codex stale-entry update test
- Replace deprecated serde_yaml with serde_yaml_ng
- Document Goose stale-entry limitation
- Quote command and args in Goose YAML snippet
- Include env/headers in server_to_json when non-empty
- Harden register_json_mcp_servers against non-object container
- Panic on unknown McpServer variants instead of silent fallback
- Remove builtin MCP server variant
- Add MCP server documentation and use type = "builtin" for builtin entries
- add MCP server registration docs to all agent detail files
- move symposium binary resolution to Symposium struct
- convert agents module to directory structure
- Remove skill nudging and activation tracking
- pacify the merciless fmt
- minor fixes — Output::is_quiet, non-interactive init, docs
- update test harness and integration tests for new hook API
- replace generic hook dispatch with agent-owned wire formats and plugin format routing
- introduce canonical symposium hook types
- Fmt and add CI check
- rename agent detail docs, add disclaimers and primary source links
- rename reference/ to agent-details/ and add cross-agent comparison
- document session-start-context in plugin definition reference
- verify hook removal when switching agents
- revert CLI naming from cargo-agents back to symposium
- verify self-contained excludes user skills from sync
- use fixtures for project plugin source tests
- add integration tests for project plugin sources and self-contained mode
- add command flow docs, agent details, and misc reference updates
- the heck
- rewrite getting-started guide for cargo-agents
- Review comments
- Allow calling hooks with other agents. Fix claude output.
- Add copilot
- Add Gemini BeforeTool hook
- Typo in PR template
- Add adapter skeleton for hooks
- Rewiew nits. Add merge docs and cfg out tests using sh
- Forward hooks output
