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
- [About symposium](./about.md)

# User's guide

- [Installing Symposium](./install.md)
- [Usage patterns](./usage-patterns.md)

# For crate authors

- [Supporting your crate](./crate-authors/supporting-your-crate.md)
- [Publishing skills](./crate-authors/publishing-skills.md)
- [Creating a plugin](./crate-authors/creating-a-plugin.md)
- [Publishing hooks](./crate-authors/publishing-hooks.md)

# Reference

- [The `cargo agents` command](./reference/cargo-agents.md)
  - [`cargo agents init`](./reference/cargo-agents-init.md)
  - [`cargo agents sync`](./reference/cargo-agents-sync.md)
  - [`cargo agents hook`](./reference/cargo-agents-hook.md)
- [Supported agents](./reference/supported-agents.md)
  - [Claude Code](./reference/agents/claude.md)
  - [GitHub Copilot](./reference/agents/copilot.md)
  - [Gemini CLI](./reference/agents/gemini.md)
  - [Codex CLI](./reference/agents/codex.md)
  - [Kiro](./reference/agents/kiro.md)
  - [OpenCode](./reference/agents/opencode.md)
  - [Goose](./reference/agents/goose.md)
- [Configuration](./reference/configuration.md)
- [Plugin definition](./reference/plugin-definition.md)
- [Skill definition](./reference/skill-definition.md)
- [Skill matching](./reference/skill-matching.md)

# Contribution guide

- [Welcome](./design/welcome.md)
- [Key repositories](./design/repositories.md)
- [Key modules](./design/module-structure.md)
- [Configuration loading](./design/configuration-loading.md)
- [Agents](./design/agents.md)
- [State](./design/state.md)
  - [Session state](./design/session-state.md)
- [Hooks](./design/hooks.md)
- [Important flows](./design/important-flows.md)
  - [`init --user`](./design/init-user-flow.md)
  - [`init --project`](./design/init-project-flow.md)
  - [`sync --workspace`](./design/sync-workspace-flow.md)
  - [`sync --agent`](./design/sync-agent-flow.md)
  - [`hook`](./design/hook-flow.md)
- [Integration test harness](./design/test-harness.md)
- [Governance](./design/governance.md)
- [Common issues](./design/common-issues.md)
- [Agent details](./design/agent-details/README.md)
  - [Claude Code](./design/agent-details/claude-code.md)
  - [GitHub Copilot](./design/agent-details/copilot.md)
  - [Gemini CLI](./design/agent-details/gemini-cli.md)
  - [Codex CLI](./design/agent-details/codex-cli.md)
  - [Goose](./design/agent-details/goose.md)
  - [Kiro](./design/agent-details/kiro.md)
  - [OpenCode](./design/agent-details/opencode.md)
