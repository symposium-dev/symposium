//! Per-session state persistence via JSON files.
//!
//! Each session gets its own file at `<config_dir>/sessions/<session-id>.json`.
//! Files are written atomically (write to temp + rename).
//!
//! State is stored as a JSON file per session under `<config_dir>/sessions/`.
//! All operations are in-memory on `SessionData`; the caller is responsible
//! for loading before and saving after mutations.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use crate::config::Symposium;

// ---------------------------------------------------------------------------
// SessionData
// ---------------------------------------------------------------------------

/// All state for a single session, serialized to/from JSON.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionData {
    /// Running prompt count, incremented on each UserPromptSubmit.
    pub prompt_count: i64,

    /// Crate names whose skills have been loaded in this session.
    pub activations: BTreeSet<String>,

    /// Nudge history: crate name → prompt count at which the last nudge was sent.
    pub nudges: BTreeMap<String, i64>,

    /// Snapshot of `.rs` file modification time taken at PreToolUse.
    /// Used to detect whether any Rust files changed during a tool use.
    pub rust_file_snapshot: BTreeMap<PathBuf, SystemTime>,

    /// Whether the agent has already been reminded to run `cargo fmt`
    /// this session. The number or reminders sent depends on the configuration.
    pub rust_fmt_reminder_sent: bool,
}

impl SessionData {
    /// Record that the agent loaded a skill for `crate_name`.
    pub fn record_activation(&mut self, crate_name: &str) {
        self.activations.insert(crate_name.to_string());
        tracing::info!(crate_name, "recorded skill activation");
    }

    /// Increment the prompt count, returning the new value.
    pub fn increment_prompt_count(&mut self) -> i64 {
        self.prompt_count += 1;
        self.prompt_count
    }

    /// Determine which of the `mentioned` crates should be nudged about,
    /// record the nudges, and return the list of crate names to include
    /// in the hook output.
    ///
    /// A crate is nudged when:
    /// - It has not been activated in this session, AND
    /// - It has never been nudged, OR enough prompts have elapsed since the last nudge.
    pub fn compute_nudges(&mut self, mentioned: &[String], nudge_interval: i64) -> Vec<String> {
        let prompt_count = self.prompt_count;
        let mut nudge_crates = Vec::new();

        for crate_name in mentioned {
            if self.activations.contains(crate_name) {
                continue;
            }

            let should_nudge = match self.nudges.get(crate_name) {
                None => true,
                Some(&last_prompt) => prompt_count - last_prompt >= nudge_interval,
            };

            if should_nudge {
                nudge_crates.push(crate_name.clone());
                self.nudges.insert(crate_name.clone(), prompt_count);
            }
        }

        nudge_crates
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Return the sessions directory, creating it if needed.
fn sessions_dir(config_dir: &Path) -> PathBuf {
    let dir = config_dir.join("sessions");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Path to a session's JSON file.
fn session_path(config_dir: &Path, session_id: &str) -> PathBuf {
    sessions_dir(config_dir).join(format!("{session_id}.json"))
}

/// Load session data from disk. Returns `Default` if the file doesn't exist
/// or can't be parsed.
pub fn load_session(sym: &Symposium, session_id: &str) -> SessionData {
    let path = session_path(sym.config_dir(), session_id);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!(error = %e, ?path, "failed to parse session file, starting fresh");
            SessionData::default()
        }),
        Err(_) => SessionData::default(),
    }
}

/// Save session data to disk atomically.
pub fn save_session(sym: &Symposium, session_id: &str, data: &SessionData) {
    let config_dir = sym.config_dir();
    let dir = sessions_dir(config_dir);
    let path = session_path(config_dir, session_id);

    let json = match serde_json::to_string_pretty(data) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(error = %e, "failed to serialize session data");
            return;
        }
    };

    // Atomic write: temp file + rename
    let tmp = dir.join(format!(".{session_id}.tmp"));
    if let Err(e) = std::fs::write(&tmp, &json) {
        tracing::warn!(error = %e, ?tmp, "failed to write session temp file");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        tracing::warn!(error = %e, ?path, "failed to rename session file");
        let _ = std::fs::remove_file(&tmp);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- SessionData tests ---

    #[test]
    fn record_activation_and_check() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.record_activation("serde");
        assert!(session.activations.contains("tokio"));
        assert!(session.activations.contains("serde"));
        assert_eq!(session.activations.len(), 2);
    }

    #[test]
    fn record_activation_is_idempotent() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.record_activation("tokio");
        assert_eq!(session.activations.len(), 1);
    }

    #[test]
    fn increment_prompt_count_works() {
        let mut session = SessionData::default();
        assert_eq!(session.increment_prompt_count(), 1);
        assert_eq!(session.increment_prompt_count(), 2);
        assert_eq!(session.increment_prompt_count(), 3);
    }

    #[test]
    fn compute_nudges_first_mention() {
        let mut session = SessionData::default();
        session.prompt_count = 1;

        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);
        assert_eq!(session.nudges["tokio"], 1);
    }

    #[test]
    fn compute_nudges_skips_activated() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.prompt_count = 1;

        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert!(result.is_empty());
    }

    #[test]
    fn compute_nudges_respects_interval() {
        let mut session = SessionData::default();

        // First nudge at prompt 1
        session.prompt_count = 1;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);

        // Too soon at prompt 10
        session.prompt_count = 10;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert!(result.is_empty());

        // Enough elapsed at prompt 51
        session.prompt_count = 51;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut session = SessionData::default();
        session.prompt_count = 5;
        session.record_activation("tokio");
        session.nudges.insert("serde".to_string(), 3);

        let json = serde_json::to_string(&session).unwrap();
        let loaded: SessionData = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.prompt_count, 5);
        assert!(loaded.activations.contains("tokio"));
        assert_eq!(loaded.nudges["serde"], 3);
    }

    // --- Persistence tests ---

    #[test]
    fn load_missing_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let data = load_session(&sym, "nonexistent");
        assert_eq!(data.prompt_count, 0);
        assert!(data.activations.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());

        let mut data = SessionData::default();
        data.prompt_count = 3;
        data.record_activation("tokio");
        data.nudges.insert("serde".to_string(), 2);

        save_session(&sym, "s1", &data);
        let loaded = load_session(&sym, "s1");

        assert_eq!(loaded.prompt_count, 3);
        assert!(loaded.activations.contains("tokio"));
        assert_eq!(loaded.nudges["serde"], 2);
    }

    #[test]
    fn save_creates_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let data = SessionData::default();
        save_session(&sym, "s1", &data);
        assert!(tmp.path().join("sessions").is_dir());
        assert!(tmp.path().join("sessions/s1.json").exists());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "not json!!!").unwrap();

        let sym = Symposium::from_dir(tmp.path());
        let data = load_session(&sym, "bad");
        assert_eq!(data.prompt_count, 0);
    }
}
