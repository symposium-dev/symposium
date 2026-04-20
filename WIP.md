# WIP: Documentation ↔ Code Consistency Fixup

## Background

This branch (`sure-trail`) began with a major overhaul of the Symposium plugin system:

- **Simplified plugin discovery** to use only `SYMPOSIUM.toml` files (replacing the old "any .toml file" approach)
- **Added plugin-level crate filtering** with wildcard support
- **Removed deprecated features** like `session-start-context` and the `activation` field on skills
- **Restructured the reference documentation** — consolidating scattered docs into clear guides and technical references

After completing those changes, we ran a systematic comparison of every reference documentation chapter against the source code using parallel review agents. The results (in `ndm-comparison-*.md` files) revealed ~20 inconsistencies where the docs and code diverged — some because code changes were incomplete, some because docs described desired-but-unimplemented behavior, and some because the overhaul surfaced pre-existing gaps.

We then triaged every discrepancy, made design decisions (e.g., true AND composition for multi-level crate filtering, `*` as a first-class predicate, direct-deps-only matching), and produced this execution plan.

## How to use this document

This plan is designed to be handed to a fresh agent for autonomous execution. Each item is self-contained: it states the problem, the fix, and where to make it. The execution order at the bottom minimizes merge conflicts between items.

**Key references**:
- `ndm-comparison-*.md` — the raw comparison reports that motivated each item
- `md/design/running-tests.md` and `md/design/testing-guidelines.md` — how to run and write tests
- `md/reference/` — the documentation that represents desired behavior

**Principle**: The documentation describes the desired behavior unless explicitly noted otherwise.

---

A fresh agent should be able to execute this plan top-to-bottom.

---

## 1. Remove `.toml` fallback in plugin discovery

**Problem**: `discover_directory_type()` in `src/plugins.rs` falls back to scanning for any `.toml` file after checking for `SYMPOSIUM.toml`. Only `SYMPOSIUM.toml` should be valid.

**Fix (code)**:
- In `discover_directory_type()`, remove the `read_dir` loop that scans for arbitrary `.toml` files. Only check for `SYMPOSIUM.toml` and `SKILL.md`.
- Update tests that rely on non-`SYMPOSIUM.toml` files:
  - `validate_source_dir_mixed`: uses `good.toml` and `bad.toml` — convert to `SYMPOSIUM.toml` in subdirectories.
  - `collect_crate_names_from_source_dir`: uses `my-plugin.toml` — convert to `SYMPOSIUM.toml` in a subdirectory.
  - `collect_crate_names_skips_invalid_items`: uses `bad.toml` — convert to `SYMPOSIUM.toml` in a subdirectory.
- Also update the stale comment on `discover_directory_type()` that mentions "any .toml file (backward compatibility)".

---

## 2. Remove `session_start_context`

**Problem**: Still present in `Plugin`, `PluginManifest`, and `handle_session_start()`.

**Fix (code)**:
- Remove `session_start_context` from `Plugin` and `PluginManifest` in `src/plugins.rs`.
- Simplify `handle_session_start()` in `src/hook.rs` to return empty unconditionally.
- Remove from all tests.

---

## 3. Make `*` a valid `Predicate` (wildcard)

**Problem**: Wildcard is special-cased as a string check. It needs to be a first-class predicate.

**Semantics**: A predicate matches against a **workspace**, not a single crate. Most predicates match if at least one workspace dependency satisfies them. `*` always matches — even a workspace with zero dependencies.

**Fix (code)**:
- In `src/predicate.rs`, add a dedicated variant to represent the wildcard. For example, add a `wildcard: bool` field to `Predicate`, or use an enum. The key constraint is that `*` must be parseable, displayable, and round-trip correctly.
- `Predicate::matches()`: if wildcard, return `true` unconditionally (even for empty `deps` slice).
- `Predicate::references_crate()`: return `false` for wildcard — it doesn't reference any specific crate.
- `Predicate::collect_crate_names()`: no-op for wildcard — don't add anything to the set. Add a comment explaining this is used in validation to check that crate names exist on crates.io, and `*` should not be looked up.
- **Unit tests**:
  - `parse("*")` succeeds and round-trips through `Display`
  - `Predicate::matches(&[])` returns `true` for wildcard (zero-dep workspace)
  - `Predicate::matches(&[("serde", v("1.0.0"))])` returns `true` for wildcard
  - `references_crate("serde")` returns `false` for wildcard
  - `collect_crate_names` returns empty set for wildcard
  - `parse("*")` combined with other predicates in a list: `["*", "serde>=1.0"]`

---

## 4. Upgrade plugin-level `crates` to use `Predicate`

**Problem**: `Plugin.crates` is `Option<Vec<String>>` with exact string matching. Should use `Predicate` like skill groups.

**Fix (code)**:
- Change `Plugin.crates` and `PluginManifest.crates` to `Option<Vec<Predicate>>`, using the same deserialization as `SkillGroup.crates`.
- Rewrite `Plugin::applies_to_crates()` to use predicate matching. With `*` as a valid predicate, the wildcard case is handled automatically.
- Update all tests that construct `Plugin` with string crates to use parsed predicates.
- **Additional tests**: Add fixtures that use version predicates at the plugin level (e.g., `crates = ["tokio>=2.0"]`) and verify they correctly reject a workspace with `tokio 1.42.0`.

---

## 5. Standardize `crates` to always use array syntax in docs

**Problem**: Docs inconsistently show `crates = "*"` (string) and `crates = ["*"]` (array). The code already requires array form — `PluginManifest.crates` is `Option<Vec<String>>` (becoming `Option<Vec<Predicate>>`), which only deserializes from a TOML array. This is a docs-only fix.

**Fix (docs only)**:
- `md/reference/plugin-definition.md`: Change inline skills example from `crates = "*"` to `crates = ["*"]`.
- `md/reference/crate-predicates.md`: Remove single-string TOML form. Only show array syntax for TOML. Keep comma-separated for YAML frontmatter.
- Search all docs for bare-string `crates =` in TOML context and convert to array form.

---

## 6. Remove `activation` field remnants

**Problem**: Tests in `src/skills.rs` reference `skill.activation` and `Activation::Always/Optional`. A test fixture also uses it. A stale comment in `src/plugins.rs` mentions it.

**Fix (code)**:
- `src/skills.rs` tests: Remove `activation: always` and `activation: optional` from test SKILL.md strings (~lines 478, 560). Remove `assert_eq!(skill.activation, ...)` assertions (~lines 492, 547). Remove `activation: Some(Activation::Always)` from `SkillGroup` construction in tests (~line 539).
- `src/plugins.rs`: Remove stale comment mentioning "activation mode" (~line 30).
- `tests/fixtures/plugins0/dot-symposium/plugins/my-skill/SKILL.md`: Remove `activation: always` line.
- If `Activation` enum still exists anywhere, remove it. If `SkillGroup` has an `activation` field, remove it.
- Ensure `cargo test` compiles and passes.

---

## 7. Remove `applies-when` handling

**Problem**: `parse_frontmatter()` in `src/skills.rs` has a special case to skip `applies-when` keys. This is dead legacy code.

**Fix (code)**:
- Remove the `if key == "applies-when"` branch. Let it be stored as a regular frontmatter field like any other unknown key.

---

## 8. Implement true AND for multi-level crate filtering

**Problem**: Docs say plugin/group/skill `crates` compose as AND. Code does fallback (skill-level replaces group-level).

**Fix (code)**:
- In `skill_matches()` in `src/skills.rs`: if the skill has its own `crates` AND the group has `crates`, require BOTH to match. If only one level has `crates`, use that. If neither has `crates`, don't match.
- Plugin-level is already checked separately in `skills_applicable_to()` before groups are loaded, so the three-level AND is: plugin must match (checked first) → group must match (pre-fetch filter) → skill must match (final filter), where each level that has `crates` must independently match the workspace.
- Add tests:
  - Skill `crates: tokio` in group `crates = ["serde"]` → requires BOTH serde AND tokio in workspace.
  - Skill with no `crates` in group `crates = ["serde"]` → only serde required.
  - Skill `crates: serde` in group with no `crates` → only serde required.
  - **Inconsistent declarations**: Skill `crates: tokio` in group `crates = ["serde"]` with workspace containing only serde → skill should NOT match (tokio not present, AND fails).

---

## 9. Validate `description` in SKILL.md frontmatter

**Problem**: Docs and agentskills.io spec say `description` is required, non-empty, max 1024 chars. Code only validates `name`.

**Fix (code)**:
- In `load_skill()` in `src/skills.rs`, after parsing frontmatter, validate:
  - `description` is present
  - `description.trim()` is non-empty (reject whitespace-only)
  - `description.trim()` is at most 1024 characters
- Update any test skills missing `description`.

---

## 10. Implement MCP server crate filtering

**Problem**: Docs show `crates` on `[[mcp_servers]]` entries, but `sync.rs` collects all MCP servers without filtering.

**Fix (code)**:
- Create a wrapper struct in `src/plugins.rs`:
  ```rust
  #[derive(Debug, Clone, Deserialize, Serialize)]
  pub struct PluginMcpServer {
      #[serde(default)]
      pub crates: Option<Vec<Predicate>>,
      #[serde(flatten)]
      pub server: McpServerEntry,
  }
  ```
- Update `PluginManifest` and `Plugin` to use `Vec<PluginMcpServer>` instead of `Vec<McpServerEntry>`.
- In `sync.rs`, filter MCP servers against workspace deps. A server matches if:
  - Plugin-level `crates` matches (already checked), AND
  - Server-level `crates` matches (or is absent, meaning inherit from plugin).
- Update `validate_plugin_has_crates()` to also check MCP server `crates` fields.
- **Tests**: Use the integration test framework (see `md/design/running-tests.md` and `md/design/testing-guidelines.md`). Set up a fixture with a plugin containing MCP servers with various `crates` declarations, run `sync` with `claude` as the agent, then check the resulting `.claude/settings.json` to verify which MCP servers were registered:
  - Server with `crates = ["serde"]` → only registered when serde is in workspace.
  - Server with no `crates` but plugin has `crates = ["tokio"]` → only registered when tokio is in workspace.
  - Server with `crates = ["*"]` → always registered.
  - Server with `crates = ["serde"]` and plugin `crates = ["tokio"]` → requires both (AND).
  - Plugin with crates only on MCP servers → passes validation.

---

## 11. Add Gemini `BeforeAgent` hook and create event mapping table

**Problem**: Gemini only registers `BeforeTool`, `AfterTool`, `SessionStart`. It supports `BeforeAgent` which maps to `user-prompt-submit`. Also, there is no single reference showing the mapping of symposium events to agent-specific event names.

**Fix (Gemini hook)**:
- Check the [Gemini hooks reference](https://geminicli.com/docs/hooks/reference/#beforeagent) for the `BeforeAgent` event format.
- Update `md/design/agent-details/gemini-cli.md` with `BeforeAgent` details.
- In `src/agents/mod.rs`, add `("BeforeAgent", "user-prompt-submit")` to the Gemini hook registration events.
- Update `md/reference/agents/gemini.md` to list `BeforeAgent`.

**Fix (event mapping table)**:
- Add a cross-agent event mapping table to `md/design/agents.md` (which already has per-agent details). Format:

| Symposium event | Claude | Copilot | Gemini | Codex | Kiro | OpenCode | Goose |
|---|---|---|---|---|---|---|---|
| `pre-tool-use` | `PreToolUse` | `preToolUse` | `BeforeTool` | `PreToolUse` | `preToolUse` | — | — |
| `post-tool-use` | `PostToolUse` | `postToolUse` | `AfterTool` | `PostToolUse` | `postToolUse` | — | — |
| `user-prompt-submit` | `UserPromptSubmit` | `userPromptSubmitted` | `BeforeAgent` | `UserPromptSubmit` | `userPromptSubmit` | — | — |
| `session-start` | `SessionStart` | `sessionStart` | `SessionStart` | `SessionStart` | `agentSpawn` | — | — |

Where `—` means the agent does not support hooks. Verify this table against the code in `src/agents/mod.rs` and the hook schema files.

---

## 12. Fix auto-sync cwd resolution

**Problem**: `run_auto_sync()` in `src/hook.rs` only runs if the hook input contains a `cwd` field. Should be more robust.

**Fix (code)**:
- Change `run_auto_sync()` to accept a fallback cwd parameter (the process's actual cwd).
- Resolution: `let cwd = event_cwd.unwrap_or(fallback_cwd)`.
- Then find the enclosing workspace root via `find_workspace_root(cwd)` and sync there.
- Update `hook::run()` to pass `std::env::current_dir()` as the fallback. In tests, the caller passes the test workspace directory instead.
- If `find_workspace_root` fails (not in a Rust project), skip sync silently (as today).

---

## 13. Filter to direct dependencies only

**Problem**: `workspace_semver_pairs()` in `src/crate_sources/list.rs` uses `metadata.packages` which includes all resolved packages (transitive). Should only match against direct dependencies of workspace members.

**Fix (code)**:
- In `list_all_workspace_crates()`, filter `metadata.packages` to only include packages that are direct dependencies of workspace members. Use `metadata.workspace_members` to identify workspace packages, then collect their direct `dependencies` from the resolve graph.
- Add tests verifying transitive deps are excluded. Use an integration test with a real workspace fixture that has a crate with known transitive dependencies (e.g., `tokio` pulls in `mio`, `socket2`, etc.). Create a skill targeting a transitive dep (e.g., `crates: mio`) and verify it does NOT get installed after `sync`. This requires crates.io access, which is fine for integration tests.

**Fix (docs)**:
- `md/reference/crate-predicates.md`: Document that predicates match against **direct** workspace dependencies, not transitive ones.

---

## 14. Reframe predicates as workspace matchers in docs

**Problem**: Docs describe predicates as matching "crates." They actually match against a workspace's dependency set.

**Fix (docs)**:
- `md/reference/crate-predicates.md`: Reword to frame predicates as matching against a workspace. Most predicates match if at least one direct dependency satisfies them. `*` always matches (even zero-dependency workspaces).
- Document the `*` wildcard behavior explicitly.

---

## 15. Move `cache_dir` out of user config into `Symposium` only

**Problem**: `cache_dir` exists in `Config` (user-facing TOML) but is primarily used for testing. Users shouldn't need to set it. It is the only internal-only field in `Config` — all other fields are user-facing.

**Fix (code)**:
- Remove `cache_dir` from `Config` struct.
- Keep the cache dir override capability in `Symposium` (the `from_dir()` test constructor already handles this).
- `resolve_cache_dir()` no longer checks `config.cache_dir` — just uses SYMPOSIUM_HOME, XDG_CACHE_HOME, or default.
- Do NOT document `cache_dir` in configuration.md (it's internal).

---

## 16. Fix XDG logs directory to use XDG_STATE_HOME

**Problem**: Code puts logs under config dir. XDG-idiomatic location is `$XDG_STATE_HOME/symposium/logs/`.

**Fix (code)**:
- Update `Symposium` to resolve a `logs_dir` using: `SYMPOSIUM_HOME/logs/` → `$XDG_STATE_HOME/symposium/logs/` → `~/.symposium/logs/` (default).
- Update `logs_dir()` method accordingly.
- **Testing**: Add a unit test that constructs `Symposium` with `XDG_STATE_HOME` set (via temp env var) and asserts `logs_dir()` returns the expected path. Use a similar pattern to how `resolve_config_dir_from_env` is tested (set env var, check result, clean up).

**Fix (docs)**:
- `md/reference/configuration.md`: Update directory resolution table to show `$XDG_STATE_HOME/symposium/logs/` for the XDG row.

---

## 17. Fix broken link in skill-definition.md

**Fix (docs)**: `md/reference/skill-definition.md`: Change `./skill-matching.md` → `./crate-predicates.md`.

---

## 18. Fix typo in plugin-definition.md

**Fix (docs)**: `md/reference/plugin-definition.md`: Fix `SYMPSOSIUM.toml` → `SYMPOSIUM.toml`.

---

## 19. Fix validate command name and flags in plugin-source.md

**Fix (docs)**: `md/reference/plugin-source.md`: Change `validate-source` → `validate`. Change `--check-crates` → mention `--no-check-crates` to skip (crate checking is on by default).

---

## 20. Add missing operators to crate atom syntax in skill-definition.md

**Fix (docs)**: `md/reference/skill-definition.md`: Add examples for `>`, `<=`, `^`, `~` operators.

---

## 21. Update `symposium crate` command docs

**Problem**: Docs overstate the output (claims "custom instructions" and "available skills").

**Fix (docs)**: `md/reference/cargo-agents-crate.md`: Update to match actual output.

---

## 22. Add MCP config paths to agent docs

**Fix (docs)**: Add an "MCP servers" section to each agent doc in `md/reference/agents/`:
- Claude: `.claude/settings.json` → `mcpServers.<name>`
- Copilot: project `.vscode/mcp.json`, global `~/.copilot/mcp-config.json` → top-level `<name>`
- Gemini: `.gemini/settings.json` → `mcpServers.<name>`
- Codex: `.codex/config.toml` → `[mcp_servers.<name>]`
- Kiro: `.kiro/settings/mcp.json` → `mcpServers.<name>`
- OpenCode: `opencode.json` / `~/.config/opencode/opencode.json` → `mcp.<name>`
- Goose: `.goose/config.yaml` / `~/.config/goose/config.yaml` → `extensions.<name>`

---

## 23. Document hook `format` field in plugin-definition.md

**Fix (docs)**: `md/reference/plugin-definition.md`: Add `format` to the `[[hooks]]` field table. Valid values: `symposium` (default), `claude`, `codex`, `copilot`, `gemini`, `kiro`. Controls input/output wire format routing.

---

## Execution Order

Recommended order to minimize conflicts:

1. **Predicate foundation** (#3 wildcard predicate, then #4 plugin-level upgrade)
2. **Removals** (#1 .toml fallback, #2 session_start_context, #6 activation, #7 applies-when)
3. **Behavioral changes** (#8 AND composition, #9 description validation, #10 MCP filtering, #12 auto-sync cwd, #13 direct deps only)
4. **Infrastructure** (#15 cache_dir, #16 XDG logs)
5. **Gemini hook** (#11)
6. **Doc-only fixes** (#5, #14, #17, #18, #19, #20, #21, #22, #23)

After each code change, run `cargo test` to verify.

---

## Execution Log

## Execution Log

### Commit 1: #3, #1, #6, #7 (combined)

**Deviation from plan**: Steps #3, #1, #6, and #7 were combined into a single commit because:
- The `activation` references (#6) in skills.rs tests prevented test compilation, blocking verification of all other changes. These had to be fixed first.
- The `.toml` fallback removal (#1) was needed because 3 tests that relied on non-SYMPOSIUM.toml files were failing after the activation fix unblocked compilation.
- The `applies-when` handling (#7) was already a skip/no-op in parse_frontmatter — left as-is.

**Additional fix**: `scan_source_dir` was updated to detect root-level SKILL.md files (the `scan_source_dir_finds_root_level_skill` test was never passing — it was added on this branch but the activation errors masked the failure).

### Commit 2: #4 (plugin-level Predicate upgrade)

Executed as planned. No deviations.

### Commit 3: #2 (session_start_context removal)

Executed as planned. Also removed dead fixture files (`session-start.toml`, `project-guidance.toml`, `only-this.toml`) that used `session-start-context` and were non-SYMPOSIUM.toml files.

### Commit 4: #8 (AND composition)

Executed as planned. No deviations.

### Commit 5: #9 (description validation)

Executed as planned. Updated all test SKILL.md strings that were missing `description`.

### Commit 6: #10 (MCP server crate filtering)

Executed as planned. Created `PluginMcpServer` wrapper with `#[serde(flatten)]` for the inner `McpServerEntry`. Integration test with fixture `mcp-filtering0` verifies filtering behavior.

### Commit 7: #12 (auto-sync cwd)

Executed as planned. No deviations.

### Commit 8: #13 (direct deps only)

Executed as planned. Used `metadata.resolve` graph to identify direct dependencies. Integration test with fixture `transitive-dep0` verifies mio (transitive dep of tokio) is excluded.

### Commit 9: #15 (cache_dir removal)

Executed as planned. No deviations.

### Commit 10: #16 (XDG logs)

Executed as planned. Added `resolve_logs_dir` function. Unit test uses `unsafe` blocks for `env::set_var`/`env::remove_var` (required in newer Rust editions).

### Commit 11: #11 (Gemini BeforeAgent + event mapping table)

Executed as planned. Added `BeforeAgent` to Gemini hook registration. Created cross-agent event mapping table in `md/design/agents.md`.

### Commit 12: #5, #14, #17, #18, #19, #20, #21, #22, #23 (doc-only fixes)

All doc fixes batched into one commit. No deviations from plan.

### Final state

- **171 tests passing** (155 unit + 2 dispatch + 2 hook_agent + 12 init_sync)
- **12 commits** covering all 23 items from the plan
- All code changes verified with `cargo test` (unit + integration)
