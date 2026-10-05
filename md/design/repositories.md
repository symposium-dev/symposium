# Key repositories

All repositories live under the [symposium-dev](https://github.com/symposium-dev) GitHub organization.

### [symposium](https://github.com/symposium-dev/symposium)

The main repository. Contains the Symposium CLI/library (Rust), the mdbook documentation, and integration tests.

### [symposium-claude-code-plugin](https://github.com/symposium-dev/symposium-claude-code-plugin)

The Claude Code plugin that connects Symposium to Claude Code. Contains a static skill, hook registrations (`PreToolUse`, `PostToolUse`, `UserPromptSubmit`), and a bootstrap script that finds or downloads the Symposium binary.

### [recommendations](https://github.com/symposium-dev/recommendations)

The central plugin repository. It holds plugins for crates that don't ship their own, and entries that make a crate's own plugin load without the consent step. Symposium fetches this as a registry by default.
