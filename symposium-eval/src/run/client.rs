//! The coding agents trials run, and where each keeps what the runner sets up or checks.
//!
//! Paths are relative to the home or the project they are found in, except managed
//! settings, which are machine-wide.

/// A coding agent trials can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Client {
    ClaudeCode,
    Codex,
}

impl Client {
    /// The name `symposium init --add-agent` takes.
    pub(crate) fn symposium_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    /// Where Symposium installs skills for this client, in the project, and where the
    /// client reads a user's own, in the home.
    pub(crate) fn skill_dir(self) -> &'static str {
        match self {
            Self::ClaudeCode => ".claude/skills",
            Self::Codex => ".agents/skills",
        }
    }

    /// The machine-wide directory of the client's managed settings, below the system
    /// root. They override a trial's own configuration.
    pub(crate) fn managed_dir(self) -> &'static str {
        match self {
            Self::ClaudeCode => "etc/claude-code",
            Self::Codex => "etc/codex",
        }
    }

    /// The files in the home that can register hooks or MCP servers.
    pub(crate) fn home_config(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[".claude/settings.json", ".claude.json"],
            Self::Codex => &[".codex/hooks.json", ".codex/config.toml"],
        }
    }

    /// The files in the home that register the client's hooks, and that the client
    /// itself leaves alone during a session.
    pub(crate) fn hook_config(self) -> &'static [&'static str] {
        match self {
            // Not `.claude.json`, which Claude Code rewrites mid-session.
            Self::ClaudeCode => &[".claude/settings.json"],
            Self::Codex => &[".codex/hooks.json"],
        }
    }

    /// The files in the project that can register hooks or MCP servers.
    pub(crate) fn project_config(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[
                ".claude/settings.json",
                ".claude/settings.local.json",
                ".mcp.json",
            ],
            Self::Codex => &[".codex/hooks.json", ".codex/config.toml"],
        }
    }
}
