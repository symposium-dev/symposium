# Key repositories

All repositories live under the [symposium-dev](https://github.com/symposium-dev) GitHub organization.

### [symposium](https://github.com/symposium-dev/symposium)

The main repository. Contains the Symposium CLI/library (Rust), the mdbook documentation, and integration tests.

### [symposium-claude-code-plugin](https://github.com/symposium-dev/symposium-claude-code-plugin)

The Claude Code plugin that connects Symposium to Claude Code. Contains a static skill (tells the agent to run `cargo agents start`), hook registrations (`PreToolUse`, `PostToolUse`, `UserPromptSubmit`), and a bootstrap script that finds or downloads the Symposium binary.

### [recommendations](https://github.com/symposium-dev/recommendations)

The central plugin repository. Crate authors submit skills and plugin manifests here. Symposium fetches this as a plugin source by default.
