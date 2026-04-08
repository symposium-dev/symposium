//! User-facing output with emoji status indicators.
//!
//! All user-visible status messages go through `Output`, which can be
//! silenced with `--quiet` or when running from a hook.

use std::path::Path;

/// Controls whether user-facing status messages are printed.
#[derive(Debug, Clone, Copy)]
pub struct Output {
    quiet: bool,
}

impl Output {
    pub fn normal() -> Self {
        Self { quiet: false }
    }

    pub fn quiet() -> Self {
        Self { quiet: true }
    }

    /// Something was already in place, no action needed.
    pub fn already_ok(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("🟢 {msg}");
        }
    }

    /// An action was taken successfully (created, added, wrote).
    pub fn done(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("✅ {msg}");
        }
    }

    /// A new item was discovered or added.
    pub fn added(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("➕ {msg}");
        }
    }

    /// An item was removed.
    pub fn removed(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("➖ {msg}");
        }
    }

    /// Informational status (not an action).
    pub fn info(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("ℹ️  {msg}");
        }
    }

    /// A warning (something went wrong but not fatal).
    pub fn warn(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            eprintln!("⚠️  {msg}");
        }
    }

    /// Print a blank line for spacing.
    pub fn blank(&self) {
        if !self.quiet {
            println!();
        }
    }

    /// Print arbitrary text (for prompts, headers, etc.).
    pub fn println(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            println!("{msg}");
        }
    }
}

/// Replace the home directory prefix with `~` for display.
pub fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}
