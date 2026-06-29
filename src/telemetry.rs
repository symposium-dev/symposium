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
    /// The kind-tagged payload. Flattened so a line reads
    /// `{"at": "...", "kind": "tool_use", ...}`.
    #[serde(flatten)]
    pub kind: EventKind,
}

/// The kind of a telemetry event and its associated data.
///
/// Anonymous by construction: no prompt text, command lines, or file paths are
/// recorded — only counts and coarse metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// An agent session began.
    SessionStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        agent: String,
        /// Plugins applicable to the workspace at session start.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        plugins: Vec<String>,
    },
    /// The user submitted a prompt.
    UserPrompt {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// The agent invoked a tool (named, but with no arguments captured).
    ToolUse {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        tool: String,
    },
}

impl TelemetryEvent {
    /// Build an event stamped at the current time.
    pub fn now(kind: EventKind) -> Self {
        Self {
            at: Utc::now(),
            kind,
        }
    }

    /// Short kind name, for summaries.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            EventKind::SessionStart { .. } => "session_start",
            EventKind::UserPrompt { .. } => "user_prompt",
            EventKind::ToolUse { .. } => "tool_use",
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
pub fn record_kind(config_dir: &Path, kind: EventKind) {
    record(config_dir, &TelemetryEvent::now(kind));
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
            if let Ok(event) = serde_json::from_str::<TelemetryEvent>(line) {
                events.push(event);
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
            kind: EventKind::ToolUse {
                session_id: Some("s1".into()),
                tool: "Bash".into(),
            },
        };
        let line = serde_json::to_string(&event).unwrap();
        assert!(line.contains(r#""kind":"tool_use""#), "line = {line}");
        assert!(line.contains(r#""tool":"Bash""#));
        let back: TelemetryEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.kind_name(), "tool_use");
    }

    #[test]
    fn records_append_and_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        record_kind(
            dir,
            EventKind::SessionStart {
                session_id: Some("s1".into()),
                agent: "claude".into(),
                plugins: vec!["tokio-plugin".into()],
            },
        );
        record_kind(
            dir,
            EventKind::UserPrompt {
                session_id: Some("s1".into()),
            },
        );
        record_kind(
            dir,
            EventKind::ToolUse {
                session_id: Some("s1".into()),
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

        record_kind(dir, EventKind::UserPrompt { session_id: None });
        let filled = status_text(dir, true);
        assert!(filled.contains("Telemetry: enabled"));
        assert!(filled.contains("1 event(s)"));
    }
}
