//! User-facing output with emoji status indicators.
//!
//! All user-visible status messages go through `Output`, which can be
//! silenced with `--quiet` or when running from a hook.

use std::path::Path;
use std::sync::{Arc, Mutex};

/// Controls whether user-facing status messages are printed.
#[derive(Debug, Clone)]
pub struct Output {
    quiet: bool,
    capture: Option<Arc<Mutex<Vec<String>>>>,
}

impl Output {
    pub fn normal() -> Self {
        Self {
            quiet: false,
            capture: None,
        }
    }

    pub fn quiet() -> Self {
        Self {
            quiet: true,
            capture: None,
        }
    }

    /// Create an output that captures all messages into a buffer
    /// instead of printing them.
    pub fn capturing() -> Self {
        Self {
            quiet: false,
            capture: Some(Arc::new(Mutex::new(Vec::new()))),
        }
    }

    /// Return all captured messages. Panics if not in capturing mode.
    pub fn captured(&self) -> Vec<String> {
        self.capture
            .as_ref()
            .expect("not in capturing mode")
            .lock()
            .unwrap()
            .clone()
    }

    pub fn is_quiet(&self) -> bool {
        self.quiet
    }

    fn emit(&self, msg: String) {
        if self.quiet {
            return;
        }
        if let Some(buf) = &self.capture {
            buf.lock().unwrap().push(msg);
        } else {
            println!("{msg}");
        }
    }

    fn emit_err(&self, msg: String) {
        if self.quiet {
            return;
        }
        if let Some(buf) = &self.capture {
            buf.lock().unwrap().push(msg);
        } else {
            eprintln!("{msg}");
        }
    }

    /// Something was already in place, no action needed.
    pub fn already_ok(&self, msg: impl std::fmt::Display) {
        self.emit(format!("🟢 {msg}"));
    }

    /// An action was taken successfully (created, added, wrote).
    pub fn done(&self, msg: impl std::fmt::Display) {
        self.emit(format!("✅ {msg}"));
    }

    /// A new item was discovered or added.
    pub fn added(&self, msg: impl std::fmt::Display) {
        self.emit(format!("➕ {msg}"));
    }

    /// An item was removed.
    pub fn removed(&self, msg: impl std::fmt::Display) {
        self.emit(format!("➖ {msg}"));
    }

    /// Informational status (not an action).
    pub fn info(&self, msg: impl std::fmt::Display) {
        self.emit(format!("ℹ️  {msg}"));
    }

    /// A warning (something went wrong but not fatal).
    pub fn warn(&self, msg: impl std::fmt::Display) {
        self.emit_err(format!("⚠️  {msg}"));
    }

    /// Print a blank line for spacing.
    pub fn blank(&self) {
        self.emit(String::new());
    }

    /// Print arbitrary text (for prompts, headers, etc.).
    pub fn println(&self, msg: impl std::fmt::Display) {
        self.emit(format!("{msg}"));
    }
}

/// Replace the home directory prefix with `~` for display.
pub fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}
