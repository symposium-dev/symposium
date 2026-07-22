# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/symposium-dev/symposium/compare/symposium-v0.4.0...symposium-v0.5.0) - 2026-07-22

### Added

- *(hook)* suggest `cargo agents --help` at session start
- *(help)* audience-grouped --help listing plugin-vended subcommands.
- *(plugins)* add subcommand manifest schema
- move skill layout to [package.metadata.symposium] in crate Cargo.toml

### Fixed

- add `self-update` to reserved subcommand names and fix typo
- make crate_metadata module pub(crate)
- address review nits on source.crate

### Other

- Canonicalize skill path consistently
- Simplify skill origin to the SKILL.md path hash
- Load crates as plugins via [[plugins]] chained references
- Dependency lists are PackageIds: CargoPm::list_deps replaces crate_pairs
- Introduce PackageId and a fetch-only PackageManager, cargo first
- Unify .agents/skills into the plugin pipeline
- Workspace plugins: the workspace's own dirs define plugins
- Add the workspace-member() predicate
- Rename crates to depends-on
- Merge pull request #253 from nikomatsakis/register-centric-plugins-rfd
- Merge pull request #254 from nikomatsakis/config-normalization
- Merge pull request #247 from abusch/push-yzlulwltuttq
- Add link in summary
- Minor updates to the blog post
- Add draft of next blog post
- Add basic telemetry infra. Add Stop hook event.
- Add RFD process with tooling and CI enforcement
- Make WorkspaceCrate and LoadedWorkspace non-exhaustive
- Address PR review feedback
- Use symposium-sdk types directly instead of re-exporting
- Require SymposiumDirs when constructing WorkspaceDeps
- Add SymposiumDirs to symposium-sdk for shared path resolution
- Add WorkspaceDeps with in-process and disk caching
- Introduce symposium-sdk crate and refactor main crate to use it
- Make sync debounce configurable; add change-detection test
- change debounce to 5sec
- Update tests and docs for sync_skill_dir changes
- Unify skill installation into sync_skill_dir
- Document that custom predicates are globally available across plugins
- Introduce CustomPredicateRegistry newtype
- Add integration tests for argument passing and witness-driven crate source
- Document and test that empty/whitespace args are not passed
- Fail predicate on any malformed witness entry
- Fail predicate when stdout is non-empty but not valid witness JSON
- Add integration tests for argument passing and witness-driven crate source
- Fix CI: replace /bin/true with script-based test helpers
- Add tests for field syntax boundary enforcement
- Reject function-call syntax in the `crates` field
- Consolidate predicate name validation and fix CrateList comma parsing
- Integrate custom predicate evaluator into PredicateContext
- Add custom predicate extensions via [[predicate]]
- Validate skills with negated crate
- Add crate predicate and lower crates field to it
- Change from shell predicates to more general predicates
- Add shell_predicates field
- Merge pull request #234 from nikomatsakis/cargo-agents-sync-verbose
- Add a sentence about global use case
- Add global install option for cargo and add env vars
- pacify the merciless fmt
- add Plugin::get_installation, capture subcommand output, snapshot tests
- pacify the merciless cargo fmt
- *(help)* hoist section headings into shared constants
- Agent help guidance:
- add the built-in dispatch step to hook-flow.md, an "Agent
- document the --help renderer and reclassify crate-info
- dispatch external `cargo agents <name>` via clap catch-all
- *(plugins)* pass empty source_name to scan_source_dir
- *(installation)* lift hook resolver into shared module
- Merge pull request #236 from nikomatsakis/skills-from-specific-crate
- make init and sync modules pub(crate)
- redesign source.crate as a nested table
- apply fmt
- improve docs
- Add `source.crate` field to decouple skill fetch target from activation predicates
- pacify the merciless fmt
- fix macOS CI test failure (symlink canonicalization)
- Also watch battery-pack.toml for sync staleness
- pacify the merciless fmt
- Skip auto-sync when Cargo.lock is unchanged
- pacify the merciless fmt
- Add user-facing hook documentation and SDK guide
- Add design tenets and hook architecture docs
- Implement per-plugin hook format selection
- Introduce symposium-hook SDK crate
- Change default crate skills path from `.symposium/skills` to `skills`

## [0.4.0](https://github.com/symposium-dev/symposium/compare/symposium-v0.3.0...symposium-v0.4.0) - 2026-05-14

### Added

- inject update nudge into session-start hook additionalContext
- add self-update command and auto-update infrastructure

### Fixed

- normalize CURRENT_VERSION in test snapshots for release compatibility
- use atomic rename in mock cargo install to avoid "Text file busy"
- rustfmt formatting and switch version check to cargo search

### Other

- remove binary download, default auto-update to on, add init prompt
- auto-update re-exec for both sync and hook invocations
- end-to-end auto-update re-exec with mock cargo install
- snapshot output for update check tests with expect_test
- capture Output messages, assert update warning text
- move auto-update check into cli::run, add integration tests
- move cargo override from env var to Symposium field
- cargo search, cargo_command() helper, tests, hook update behavior
- SkillOrigin keyed on source location, readable Crate dirs
- demotion to suffixed names when a new origin introduces a conflict
- promotion to unsuffixed slot when origin conflict disappears
- only suffix skill dirs with origin hash on conflict
- dispatch on group source via single match in load_skills_for_group
- SkillOrigin::Git keys on (repo, commit_sha, skill_path)
- per-group disambiguator on SkillOrigin::Plugin; sha2 hash
- introduce SkillOrigin and dedup skill installs by origin
- add has_symposium_marker helper
- address review comments
- drop unimplemented 'distribution: workspace' section
- rewrite workspace-skills page
- Document workspace skills in user guide
- Add agents-syncing: mirror user-authored skills across agent dirs

## [0.3.0](https://github.com/symposium-dev/symposium/compare/symposium-v0.2.1...symposium-v0.3.0) - 2026-05-13

### Added

- track installed skills via per-skill `.symposium` marker file
- resolve crate-sourced skills during sync
- add matched_crates predicate resolution
- add crate_path skill source type with parse-time validation

### Fixed

- resolve path dependencies in crate-info command
- *(doc)* Add GitHub and Zulip links.

### Other

- Merge pull request #216 from nikomatsakis/gitignore-strategy
- pacify the merciless cargo-fmt
- rename publishing-skills.md to authoring-a-plugin.md; add mdbook redirects
- extract normalize_crate_name helper for hyphen/underscore equality
- introduce CratePathSource newtype for the CratePath payload
- make PluginSource an enum preserving shorthand vs explicit crate_path
- rewrite crate-author documentation for crate-sourced skills
- remove agent-specific directories
- update stale `crate` subcommand to `crate-info`
- Merge pull request #203 from jlizen/clippy/collapsible-if-and-plugins-misc
- Merge pull request #205 from jlizen/clippy/trivial-mechanical
- Merge pull request #202 from jlizen/main
- Refactor installation schema, again
- Refactor hook installation schema
- Add ability to resolve hooks using distributions.
- Merge pull request #192 from anaslimem/claude-updated-input-json

## [0.2.1](https://github.com/symposium-dev/symposium/compare/symposium-v0.2.0...symposium-v0.2.1) - 2026-04-21

### Other

- Add authorship, fix typo, and link to foundation blog
- Fix RTK link
- Merge pull request #186 from symposium-dev/initial-blog
- Apply suggestions from code review
- suggested copy edits
- Update homepage
- Add blog with initial post

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

- update the name of the book
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
- Review nits. Add merge docs and cfg out tests using sh
- Forward hooks output
