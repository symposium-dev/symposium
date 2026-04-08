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

- [The `symposium` command](./reference/symposium.md)
  - [`symposium init`](./reference/symposium-init.md)
  - [`symposium sync`](./reference/symposium-sync.md)
  - [`symposium hook`](./reference/symposium-hook.md)
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
- [Important flows](./design/important-flows.md)
  - [`init --user`](./design/init-user-flow.md)
  - [`init --project`](./design/init-project-flow.md)
  - [`sync --workspace`](./design/sync-workspace-flow.md)
  - [`sync --agent`](./design/sync-agent-flow.md)
  - [`hook`](./design/hook-flow.md)
- [Integration test harness](./design/test-harness.md)
- [Governance](./design/governance.md)
- [Reference material](./design/reference/README.md)
  - [Claude Code hooks](./design/reference/claude-code-hooks.md)
  - [GitHub Copilot hooks](./design/reference/copilot-hooks.md)
  - [Gemini CLI hooks](./design/reference/gemini-cli-hooks.md)
