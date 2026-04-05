//! Per-session state persistence via JSON files.
//!
//! Each session gets its own file at `<config_dir>/sessions/<session-id>.json`.
//! Files are written atomically (write to temp + rename).

pub mod session;

use std::path::{Path, PathBuf};

use session::SessionData;

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
pub fn load_session(config_dir: &Path, session_id: &str) -> SessionData {
    let path = session_path(config_dir, session_id);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!(error = %e, ?path, "failed to parse session file, starting fresh");
            SessionData::default()
        }),
        Err(_) => SessionData::default(),
    }
}

/// Save session data to disk atomically.
pub fn save_session(config_dir: &Path, session_id: &str, data: &SessionData) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let data = load_session(tmp.path(), "nonexistent");
        assert_eq!(data.prompt_count, 0);
        assert!(data.activations.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();

        let mut data = SessionData::default();
        data.prompt_count = 3;
        data.record_activation("tokio");
        data.nudges.insert("serde".to_string(), 2);

        save_session(tmp.path(), "s1", &data);
        let loaded = load_session(tmp.path(), "s1");

        assert_eq!(loaded.prompt_count, 3);
        assert!(loaded.activations.contains("tokio"));
        assert_eq!(loaded.nudges["serde"], 2);
    }

    #[test]
    fn save_creates_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data = SessionData::default();
        save_session(tmp.path(), "s1", &data);
        assert!(tmp.path().join("sessions").is_dir());
        assert!(tmp.path().join("sessions/s1.json").exists());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "not json!!!").unwrap();

        let data = load_session(tmp.path(), "bad");
        assert_eq!(data.prompt_count, 0);
    }
}
