//! Hook logic for reminding the agent to run `cargo fmt` after Rust files change.
//!
//! Rather than running the formatter directly, we inject a suggestion into the agent's context
//! via `HookOutput`. The reminder is sent according to the configured `fmt-reminder` policy.

use crate::{config::FormatReminderPolicy, session_state::SessionData};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::SystemTime,
};
use walkdir::WalkDir;

/// Context passed to the format hook functions.
pub struct RustFmtHookContext {
    /// Directory where the tool ran.
    pub workdir: PathBuf,
}

/// Snapshot the modification times of all `*.rs` files found recursively
/// under `cwd`. Stored in session state at the end of each `PostToolUse`
/// and compared at the start of the next one.
pub fn snapshot_rust_files(cwd: &Path) -> BTreeMap<PathBuf, SystemTime> {
    let mut mtimes = BTreeMap::new();
    collect_rust_file_mtimes(cwd, &mut mtimes);
    mtimes
}

/// Walk `dir` recursively, collecting mtimes(modification times) of all `*.rs` files.
pub fn collect_rust_file_mtimes(dir: &Path, mtimes: &mut BTreeMap<PathBuf, SystemTime>) {
    for entry in WalkDir::new(dir).into_iter().flat_map(|dir| dir.ok()) {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            if let Ok(metadata) = path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    mtimes.insert(path.to_path_buf(), modified);
                }
            }
        }
    }
}

/// Check whether any `*.rs` files under `cwd` have changed compared to
/// the snapshot stored in session state. Returns `true` if any file was
/// added, removed, or modified.
pub fn rust_files_changed_since(cwd: &Path, previous: &BTreeMap<PathBuf, SystemTime>) -> bool {
    let current = snapshot_rust_files(cwd);

    if current.len() != previous.len() {
        return true;
    }

    // Any new or modified files?
    for (path, mtime) in &current {
        match previous.get(path) {
            Some(prev) if prev != mtime => return true, // modified
            None => return true,                        // new file
            _ => {}
        }
    }

    false
}

/// Called at `PostToolUse`. Returns a suggestion string if the agent should
/// be reminded to run `cargo fmt`, or `None` if no reminder is needed.
///
/// A reminder is sent when:
///   - At least one `*.rs` file changed since the last `PostToolUse`, AND
///   - The configured `fmt-reminder` policy allows it.
///
/// The session state is updated regardless — the snapshot is refreshed so
/// subsequent tool uses compare against the latest state (unless the policy is Never).
pub fn maybe_suggest_rust_fmt(
    session: &mut SessionData,
    cwd: &Path,
    policy: &FormatReminderPolicy,
) -> Option<String> {
    // Optimization: If the policy is never, don't bother walking the file system.
    if matches!(policy, FormatReminderPolicy::Never) {
        return None;
    }

    let changed = rust_files_changed_since(cwd, &session.rust_file_snapshot);

    // Refresh snapshot for the next tool use
    session.rust_file_snapshot = snapshot_rust_files(cwd);

    if !changed {
        return None;
    }

    match policy {
        // Reminder is sent at most once per session.
        // TODO: make the threshold configurable (e.g. every N tool uses)
        FormatReminderPolicy::Once => {
            if session.rust_fmt_reminder_sent {
                return None;
            }
            session.rust_fmt_reminder_sent = true;
            Some(rust_fmt_suggestion_text())
        }
        FormatReminderPolicy::Always => Some(rust_fmt_suggestion_text()),
        FormatReminderPolicy::Never => None,
    }
}

pub fn rust_fmt_suggestion_text() -> String {
    "One or more Rust source files were modified.\n\
     Please run `cargo fmt` to keep the code consistently formatted."
        .to_string()
}
