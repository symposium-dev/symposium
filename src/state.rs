//! Persistent state: `state.toml` in the config directory.
//!
//! Tracks the version of the binary that last touched the config/cache
//! directories and the last time an update check was performed.
//! Future versions can use a version mismatch to trigger migrations.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "state.toml";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Runtime version of this binary, baked in at compile time.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize, Serialize)]
pub struct State {
    /// Semver of the binary that last wrote this file.
    pub version: String,

    /// Last time we checked crates.io for a newer version.
    #[serde(default, rename = "last-update-check")]
    pub last_update_check: Option<DateTime<Utc>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION.to_string(),
            last_update_check: None,
        }
    }
}

/// Load `state.toml` from `config_dir`, returning `None` if absent or unparseable.
pub fn load(config_dir: &Path) -> Option<State> {
    let path = state_path(config_dir);
    let contents = fs::read_to_string(path).ok()?;
    toml::from_str(&contents).ok()
}

fn save(config_dir: &Path, state: &State) {
    if let Ok(contents) = toml::to_string_pretty(state) {
        let _ = fs::write(state_path(config_dir), contents);
    }
}

/// Write `state.toml` into `config_dir` with the current binary version,
/// preserving other fields.
pub fn stamp(config_dir: &Path) {
    let mut state = load(config_dir).unwrap_or_default();
    state.version = CURRENT_VERSION.to_string();
    save(config_dir, &state);
}

/// Ensure `state.toml` exists and reflects the running binary version.
///
/// Returns the previously recorded version (if any) so callers can
/// decide whether a migration is needed.
pub fn ensure_current(config_dir: &Path) -> Option<String> {
    let prev = load(config_dir);
    let prev_version = prev.as_ref().map(|s| s.version.clone());

    let needs_write = match &prev {
        None => true,
        Some(s) => s.version != CURRENT_VERSION,
    };

    if needs_write {
        stamp(config_dir);
    }

    prev_version
}

/// Whether enough time has elapsed since the last update check.
pub fn should_check_for_update(config_dir: &Path) -> bool {
    let Some(state) = load(config_dir) else {
        return true;
    };
    let Some(last_check) = state.last_update_check else {
        return true;
    };
    let elapsed = Utc::now().signed_duration_since(last_check);
    elapsed.to_std().unwrap_or(Duration::ZERO) >= UPDATE_CHECK_INTERVAL
}

/// Record that an update check just happened.
pub fn record_update_check(config_dir: &Path) {
    let mut state = load(config_dir).unwrap_or_default();
    state.last_update_check = Some(Utc::now());
    save(config_dir, &state);
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load(tmp.path()).is_none());
        stamp(tmp.path());
        let state = load(tmp.path()).expect("should exist after stamp");
        assert_eq!(state.version, CURRENT_VERSION);
    }

    #[test]
    fn ensure_current_returns_none_on_first_run() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = ensure_current(tmp.path());
        assert!(prev.is_none());
        let state = load(tmp.path()).expect("should be written");
        assert_eq!(state.version, CURRENT_VERSION);
    }

    #[test]
    fn ensure_current_returns_previous_version() {
        let tmp = tempfile::tempdir().unwrap();
        let old = State {
            version: "0.1.0".to_string(),
            last_update_check: None,
        };
        fs::write(
            tmp.path().join(STATE_FILE),
            toml::to_string_pretty(&old).unwrap(),
        )
        .unwrap();

        let prev = ensure_current(tmp.path());
        assert_eq!(prev.as_deref(), Some("0.1.0"));
        let state = load(tmp.path()).expect("should be updated");
        assert_eq!(state.version, CURRENT_VERSION);
    }

    #[test]
    fn should_check_when_no_state() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(should_check_for_update(tmp.path()));
    }

    #[test]
    fn should_check_when_never_checked() {
        let tmp = tempfile::tempdir().unwrap();
        stamp(tmp.path());
        assert!(should_check_for_update(tmp.path()));
    }

    #[test]
    fn should_not_check_when_recently_checked() {
        let tmp = tempfile::tempdir().unwrap();
        record_update_check(tmp.path());
        assert!(!should_check_for_update(tmp.path()));
    }

    #[test]
    fn should_check_when_interval_elapsed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = State::default();
        state.last_update_check = Some(Utc::now() - chrono::Duration::hours(25));
        save(tmp.path(), &state);
        assert!(should_check_for_update(tmp.path()));
    }

    #[test]
    fn stamp_preserves_last_update_check() {
        let tmp = tempfile::tempdir().unwrap();
        record_update_check(tmp.path());
        let before = load(tmp.path()).unwrap().last_update_check;
        assert!(before.is_some());

        stamp(tmp.path());
        let after = load(tmp.path()).unwrap().last_update_check;
        assert_eq!(before, after);
    }
}
