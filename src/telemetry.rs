//! Opt-in usage telemetry: local event log.
//!
//! Telemetry is **opt-in** (`[telemetry] enabled = true`), **per-user**, and
//! **local-first**: events are appended as JSON lines under
//! `<config-dir>/telemetry/`, one file per UTC day. Nothing is uploaded — that
//! is a separate step the user takes deliberately (e.g. `cargo agents telemetry
//! show`).
//!
//! Older daily files are rolled off after [`RETENTION_DAYS`].
//!
//! Every entry point here is best-effort: a failure to read or write the log
//! must never break a hook, so errors are logged and swallowed.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Daily event files older than this are deleted on roll-off.
pub const RETENTION_DAYS: i64 = 30;

/// A single telemetry event: a timestamp plus a kind-tagged payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    /// When the event occurred (UTC).
    pub at: DateTime<Utc>,
    /// The session this event belongs to. Common to every kind, so it rides on
    /// the envelope; `None` when the agent supplies no id (e.g. Copilot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The kind-tagged payload. Flattened so a line reads
    /// `{"at": "...", "kind": "tool_use", ...}`.
    #[serde(flatten)]
    pub kind: EventKind,
}

/// The kind of a telemetry event and its associated data.
///
/// Anonymous by construction: no prompt text, command lines, or file paths are
/// recorded — only counts and coarse metadata.
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// An agent session began.
    SessionStart {
        agent: String,
        /// `None` when no sync ran (auto-sync off): we record the absence
        /// rather than guess a count.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        crate_count: Option<usize>,
    },
    /// The user submitted a prompt.
    UserPrompt,
    /// The agent invoked a tool (named, but with no arguments captured).
    ToolUse { tool: String },
    /// A plugin applied to the workspace.
    PluginActivation {
        plugin: String,
        /// Only the crates that satisfied the plugin's predicates, so this is
        /// empty for wildcard / env / shell gates.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        crates: Vec<String>,
    },
    /// A skill applied to the workspace.
    SkillActivation {
        skill: String,
        /// `None` for a standalone SKILL.md that no plugin vends.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plugin: Option<String>,
        /// Unioned across the plugin, group, and skill predicate levels; empty
        /// for wildcard gates.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        crates: Vec<String>,
    },
    /// A plugin hook ran in response to an event.
    HookInvocation {
        hook: String,
        plugin: String,
        duration_ms: u64,
    },
    /// A sync pass over the workspace skills.
    SyncRun {
        installed: usize,
        reaped: usize,
        plugins_matched: usize,
    },
    /// The agent finished a turn (end of response).
    Stop,
}

impl TelemetryEvent {
    /// Build an event stamped at the current time.
    pub fn now(session_id: Option<String>, kind: EventKind) -> Self {
        Self {
            at: Utc::now(),
            session_id,
            kind,
        }
    }

    /// Short kind name, for summaries.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            EventKind::SessionStart { .. } => "session_start",
            EventKind::UserPrompt => "user_prompt",
            EventKind::ToolUse { .. } => "tool_use",
            EventKind::PluginActivation { .. } => "plugin_activation",
            EventKind::SkillActivation { .. } => "skill_activation",
            EventKind::HookInvocation { .. } => "hook_invocation",
            EventKind::SyncRun { .. } => "sync_run",
            EventKind::Stop => "stop",
        }
    }
}

/// Directory holding the per-day JSONL event files.
pub fn telemetry_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("telemetry")
}

fn file_for(dir: &Path, at: DateTime<Utc>) -> PathBuf {
    dir.join(format!("events-{}.jsonl", at.format("%Y-%m-%d")))
}

/// Append one event to today's log file. Best-effort.
pub fn record(config_dir: &Path, event: &TelemetryEvent) {
    let dir = telemetry_dir(config_dir);
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::debug!(error = %e, "telemetry: failed to create telemetry dir");
        return;
    }
    let line = match serde_json::to_string(event) {
        Ok(mut s) => {
            s.push('\n');
            s
        }
        Err(e) => {
            tracing::debug!(error = %e, "telemetry: failed to serialize event");
            return;
        }
    };
    let path = file_for(&dir, event.at);
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(line.as_bytes()) {
                tracing::debug!(error = %e, "telemetry: failed to append event");
            }
        }
        Err(e) => tracing::debug!(error = %e, "telemetry: failed to open event file"),
    }
}

/// Convenience: stamp `kind` with the current time and record it.
pub fn record_kind(config_dir: &Path, session_id: Option<String>, kind: EventKind) {
    record(config_dir, &TelemetryEvent::now(session_id, kind));
}

/// Parse the date out of an `events-YYYY-MM-DD.jsonl` filename.
fn file_date(path: &Path) -> Option<NaiveDate> {
    let stem = path.file_name()?.to_str()?;
    let date = stem.strip_prefix("events-")?.strip_suffix(".jsonl")?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

/// Delete daily files older than `retention_days`. Best-effort.
pub fn roll_off(config_dir: &Path, retention_days: i64) {
    let dir = telemetry_dir(config_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let cutoff = Utc::now().date_naive() - chrono::Duration::days(retention_days);
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(date) = file_date(&path)
            && date < cutoff
            && let Err(e) = fs::remove_file(&path)
        {
            tracing::debug!(error = %e, "telemetry: failed to roll off old file");
        }
    }
}

/// All event files, sorted by date (oldest first).
fn event_files(config_dir: &Path) -> Vec<PathBuf> {
    let dir = telemetry_dir(config_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| file_date(p).is_some())
        .collect();
    files.sort_by_key(|p| file_date(p));
    files
}

/// Read all recorded events, oldest first.
pub fn read_events(config_dir: &Path) -> Vec<TelemetryEvent> {
    let mut events = Vec::new();
    for path in event_files(config_dir) {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        for line in contents.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TelemetryEvent>(line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    tracing::debug!(error = %e, "telemetry: skipping unparseable event line")
                }
            }
        }
    }
    events
}

/// The most recent `limit` events, oldest first within the returned slice.
pub fn recent_events(config_dir: &Path, limit: usize) -> Vec<TelemetryEvent> {
    let mut events = read_events(config_dir);
    if events.len() > limit {
        events.drain(0..events.len() - limit);
    }
    events
}

/// How much telemetry is stored locally — for `telemetry status`.
#[derive(Debug, Default)]
pub struct Usage {
    pub files: usize,
    pub events: usize,
    pub bytes: u64,
    pub oldest: Option<NaiveDate>,
    pub newest: Option<NaiveDate>,
}

/// Summarize what is stored on disk.
pub fn usage(config_dir: &Path) -> Usage {
    let files = event_files(config_dir);
    let mut u = Usage {
        files: files.len(),
        ..Usage::default()
    };
    for path in &files {
        if let Ok(meta) = fs::metadata(path) {
            u.bytes += meta.len();
        }
        if let Ok(contents) = fs::read_to_string(path) {
            u.events += contents.lines().filter(|l| !l.trim().is_empty()).count();
        }
        if let Some(date) = file_date(path) {
            u.oldest = Some(u.oldest.map_or(date, |o| o.min(date)));
            u.newest = Some(u.newest.map_or(date, |n| n.max(date)));
        }
    }
    u
}

/// Render `telemetry status` output.
pub fn status_text(config_dir: &Path, enabled: bool) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Telemetry: {}",
        if enabled { "enabled" } else { "disabled" }
    );
    let _ = writeln!(
        out,
        "  data directory: {}",
        telemetry_dir(config_dir).display()
    );
    let u = usage(config_dir);
    if u.events == 0 {
        let _ = writeln!(out, "  stored:         nothing yet");
    } else {
        let _ = writeln!(
            out,
            "  stored:         {} file(s), {} event(s), {}",
            u.files,
            u.events,
            human_bytes(u.bytes)
        );
        if let (Some(o), Some(n)) = (u.oldest, u.newest) {
            let _ = writeln!(out, "  range:          {o} … {n}");
        }
    }
    out
}

fn human_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_round_trips_as_jsonl() {
        let event = TelemetryEvent {
            at: DateTime::parse_from_rfc3339("2026-06-23T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            session_id: Some("s1".into()),
            kind: EventKind::ToolUse {
                tool: "Bash".into(),
            },
        };
        let line = serde_json::to_string(&event).unwrap();
        assert!(line.contains(r#""kind":"tool_use""#), "line = {line}");
        assert!(line.contains(r#""tool":"Bash""#));
        assert!(line.contains(r#""session_id":"s1""#), "line = {line}");
        let back: TelemetryEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.session_id.as_deref(), Some("s1"));
        assert_eq!(back.kind_name(), "tool_use");
    }

    #[test]
    fn skill_activation_round_trips_with_witness_crates() {
        let event = TelemetryEvent {
            at: DateTime::parse_from_rfc3339("2026-07-09T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            session_id: Some("s1".into()),
            kind: EventKind::SkillActivation {
                skill: "example-skill".into(),
                plugin: Some("example-plugin".into()),
                crates: vec!["acme-core".into(), "acme-io".into()],
            },
        };
        let line = serde_json::to_string(&event).unwrap();
        assert!(
            line.contains(r#""kind":"skill_activation""#),
            "line = {line}"
        );
        assert!(line.contains(r#""crates":["acme-core","acme-io"]"#));
        let back: TelemetryEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.kind_name(), "skill_activation");
    }

    #[test]
    fn every_kind_round_trips_with_matching_tag() {
        let kinds = [
            EventKind::SessionStart {
                agent: "claude".into(),
                crate_count: Some(1),
            },
            EventKind::SessionStart {
                agent: "copilot".into(),
                crate_count: None,
            },
            EventKind::UserPrompt,
            EventKind::ToolUse {
                tool: "Bash".into(),
            },
            EventKind::PluginActivation {
                plugin: "example-plugin".into(),
                crates: vec!["acme-core".into()],
            },
            EventKind::SkillActivation {
                skill: "example-skill".into(),
                plugin: None,
                crates: vec![],
            },
            EventKind::HookInvocation {
                hook: "format-check".into(),
                plugin: "example-plugin".into(),
                duration_ms: 5,
            },
            EventKind::SyncRun {
                installed: 1,
                reaped: 0,
                plugins_matched: 2,
            },
            EventKind::Stop,
        ];
        for kind in kinds {
            let event = TelemetryEvent::now(Some("s1".into()), kind);
            let value = serde_json::to_value(&event).unwrap();
            assert_eq!(
                value["kind"].as_str(),
                Some(event.kind_name()),
                "kind_name() drifted from the serde tag: {value}"
            );
            let line = serde_json::to_string(&event).unwrap();
            let back: TelemetryEvent = serde_json::from_str(&line).unwrap();
            assert_eq!(back.kind_name(), event.kind_name(), "line = {line}");
            assert_eq!(back.session_id.as_deref(), Some("s1"), "line = {line}");
        }
    }

    #[test]
    fn empty_witness_crates_are_omitted() {
        // A wildcard skill (empty witness) should serialize without a `crates`
        // key at all, not as `"crates":[]`.
        let event = TelemetryEvent::now(
            None,
            EventKind::SkillActivation {
                skill: "wildcard-skill".into(),
                plugin: None,
                crates: vec![],
            },
        );
        let line = serde_json::to_string(&event).unwrap();
        assert!(!line.contains("crates"), "line = {line}");
        assert!(!line.contains("plugin"), "line = {line}");
        assert!(!line.contains("session_id"), "line = {line}");
    }

    #[test]
    fn records_append_and_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        record_kind(
            dir,
            Some("s1".into()),
            EventKind::SessionStart {
                agent: "claude".into(),
                crate_count: Some(3),
            },
        );
        record_kind(dir, Some("s1".into()), EventKind::UserPrompt);
        record_kind(
            dir,
            Some("s1".into()),
            EventKind::ToolUse {
                tool: "Bash".into(),
            },
        );

        let events = read_events(dir);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind_name(), "session_start");
        assert_eq!(events[2].kind_name(), "tool_use");

        let u = usage(dir);
        assert_eq!(u.files, 1);
        assert_eq!(u.events, 3);
        assert!(u.bytes > 0);
    }

    #[test]
    fn read_events_skips_unparseable_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = telemetry_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        let file = file_for(&dir, Utc::now());
        // A good line, a corrupt one, and a blank one: only the good line reads.
        fs::write(
            &file,
            "{\"at\":\"2026-07-09T10:00:00Z\",\"kind\":\"user_prompt\"}\nnot json\n\n",
        )
        .unwrap();

        let events = read_events(tmp.path());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind_name(), "user_prompt");
    }

    #[test]
    fn roll_off_removes_old_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = telemetry_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        // One file well past retention, one recent.
        let old = dir.join("events-2000-01-01.jsonl");
        let recent = file_for(&dir, Utc::now());
        fs::write(&old, "{}\n").unwrap();
        fs::write(&recent, "{}\n").unwrap();

        roll_off(tmp.path(), RETENTION_DAYS);

        assert!(!old.exists(), "old file should be removed");
        assert!(recent.exists(), "recent file should remain");
    }

    #[test]
    fn status_text_reflects_enabled_and_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let empty = status_text(dir, false);
        assert!(empty.contains("Telemetry: disabled"));
        assert!(empty.contains("nothing yet"));

        record_kind(dir, None, EventKind::UserPrompt);
        let filled = status_text(dir, true);
        assert!(filled.contains("Telemetry: enabled"));
        assert!(filled.contains("1 event(s)"));
    }
}
