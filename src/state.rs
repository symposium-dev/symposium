//! Persistent state: `state.toml` in the config directory.
//!
//! Tracks the version of the binary that last touched the config/cache
//! directories, the last time an update check was performed, and the MCP
//! server entries Symposium wrote into agent configs.
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

#[derive(Debug, Serialize)]
pub struct State {
    /// Semver of the binary that last wrote this file.
    pub version: String,

    /// Last time we checked crates.io for a newer version.
    #[serde(default, rename = "last-update-check")]
    pub last_update_check: Option<DateTime<Utc>>,
    #[serde(default, rename = "detected-agents")]
    pub detected_agents: Vec<String>,
    /// None means choices predate explicit-choice tracking and need migration.
    #[serde(
        default,
        rename = "explicit-agents",
        skip_serializing_if = "Option::is_none"
    )]
    pub explicit_agents: Option<Vec<String>>,

    /// MCP server entries Symposium wrote. Removal is limited to these, since
    /// a server name alone may also belong to an entry the user wrote.
    #[serde(rename = "mcp-server")]
    pub mcp_servers: Vec<McpRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRegistration {
    pub agent: String,
    pub file: PathBuf,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct RawState {
    version: String,
    #[serde(default, rename = "last-update-check")]
    last_update_check: Option<DateTime<Utc>>,
    #[serde(default, rename = "detected-agents")]
    detected_agents: Vec<String>,
    #[serde(default, rename = "explicit-agents")]
    explicit_agents: Option<Vec<String>>,
    #[serde(default, rename = "mcp-server")]
    mcp_servers: Vec<McpRegistration>,
}

impl RawState {
    fn validate(self) -> State {
        State {
            version: self.version,
            last_update_check: self.last_update_check,
            detected_agents: self.detected_agents,
            explicit_agents: self.explicit_agents,
            mcp_servers: self.mcp_servers,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION.to_string(),
            last_update_check: None,
            detected_agents: Vec::new(),
            explicit_agents: None,
            mcp_servers: Vec::new(),
        }
    }
}

/// Load `state.toml` from `config_dir`, returning `None` if absent or unparseable.
pub fn load(config_dir: &Path) -> Option<State> {
    let path = state_path(config_dir);
    let contents = fs::read_to_string(path).ok()?;
    toml::from_str::<RawState>(&contents)
        .ok()
        .map(RawState::validate)
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

pub fn mcp_registrations(config_dir: &Path) -> Vec<McpRegistration> {
    load(config_dir)
        .map(|state| state.mcp_servers)
        .unwrap_or_default()
}

pub fn record_mcp_registrations(config_dir: &Path, registrations: Vec<McpRegistration>) {
    let mut state = load(config_dir).unwrap_or_default();
    let before = state.mcp_servers.len();
    for registration in registrations {
        if !state.mcp_servers.contains(&registration) {
            state.mcp_servers.push(registration);
        }
    }
    if state.mcp_servers.len() != before {
        save(config_dir, &state);
    }
}

pub fn forget_mcp_registrations(config_dir: &Path, removed: &[McpRegistration]) {
    if removed.is_empty() {
        return;
    }
    let mut state = load(config_dir).unwrap_or_default();
    state.mcp_servers.retain(|r| !removed.contains(r));
    save(config_dir, &state);
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE)
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_choice_history_preserves_other_state_and_distinguishes_migration() {
        let dir = tempfile::tempdir().unwrap();
        super::stamp(dir.path());
        assert!(super::load(dir.path()).unwrap().explicit_agents.is_none());
        super::record_update_check(dir.path());
        let registration = super::McpRegistration {
            agent: "claude".into(),
            file: dir.path().join("mcp.json"),
            name: "test".into(),
        };
        let mut state = super::load(dir.path()).unwrap();
        state.mcp_servers.push(registration.clone());
        super::save(dir.path(), &state);
        super::record_agent_choices(dir.path(), vec![], vec!["codex".into()]);
        super::stamp(dir.path());
        let state = super::load(dir.path()).unwrap();
        assert_eq!(state.explicit_agents, Some(vec![]));
        assert_eq!(state.detected_agents, vec!["codex"]);
        assert!(state.last_update_check.is_some());
        assert_eq!(state.mcp_servers, vec![registration]);
    }

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
            ..State::default()
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
        let state = State {
            last_update_check: Some(Utc::now() - chrono::Duration::hours(25)),
            ..State::default()
        };
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

    #[test]
    fn mcp_registrations_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let registrations = vec![
            McpRegistration {
                agent: "claude".to_string(),
                file: PathBuf::from("/home/me/.claude.json"),
                name: "demo".to_string(),
            },
            McpRegistration {
                agent: "codex".to_string(),
                file: PathBuf::from("/home/me/.codex/config.toml"),
                name: "other".to_string(),
            },
        ];
        let state = State {
            mcp_servers: registrations.clone(),
            ..State::default()
        };
        save(tmp.path(), &state);
        assert_eq!(load(tmp.path()).unwrap().mcp_servers, registrations);

        forget_mcp_registrations(tmp.path(), &registrations[..1]);
        assert_eq!(mcp_registrations(tmp.path()), registrations[1..]);
    }

    #[test]
    fn state_without_mcp_registrations_loads_empty() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(STATE_FILE), "version = \"0.1.0\"\n").unwrap();
        assert!(load(tmp.path()).unwrap().mcp_servers.is_empty());
    }
}

pub fn record_detected_agents(config_dir: &Path, agents: Vec<String>) {
    let mut state = load(config_dir).unwrap_or_default();
    state.detected_agents = agents;
    save(config_dir, &state);
}

/// Persist selection provenance without disturbing update checks or MCP registrations.
pub fn record_agent_choices(config_dir: &Path, explicit: Vec<String>, detected: Vec<String>) {
    let mut state = load(config_dir).unwrap_or_default();
    state.explicit_agents = Some(explicit);
    state.detected_agents = detected;
    save(config_dir, &state);
}
