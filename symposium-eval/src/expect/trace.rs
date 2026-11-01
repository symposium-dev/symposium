//! Whether Symposium's `SessionStart` hook ran and succeeded (RFD reference,
//! "Synchronization and hooks").
//!
//! With `SYMPOSIUM_HOOK_TRACE` set, Symposium appends one JSON line for each hook call
//! that reached a response, naming the event, the agent, and the exit code. A call that
//! failed before responding leaves no line, so a missing `SessionStart` fails. The agent
//! can write where the trace is, so the reader follows no link and holds at most a
//! bounded line; a test runs the real hook and reads its trace.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::Path;

use serde::Deserialize;

use super::Verdict;
use crate::jsonl::{self, Line};

/// What the trace showed about the client's `SessionStart` hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceEvidence {
    /// A call that exited 0.
    Succeeded,
    /// Only calls that exited otherwise.
    Failed,
    /// No call, and nothing that could be one.
    Missing,
    /// No successful call, and a line or file that could not be read as one.
    Unknown,
}

impl TraceEvidence {
    /// A trial starts with no trace, so a missing call means the hook never ran.
    pub(crate) fn verdict(self) -> Verdict {
        match self {
            Self::Succeeded => Verdict::Pass,
            Self::Failed | Self::Missing => Verdict::Fail,
            Self::Unknown => Verdict::Inconclusive,
        }
    }
}

/// The part of a trace line this reader relies on; Symposium writes more beside it.
#[derive(Debug, Deserialize)]
struct Entry {
    event: String,
    agent: String,
    exit_code: u8,
}

/// Larger than any trace a session leaves. Each line carries a hook's input, which for
/// a tool hook holds the tool's result, so a long session's trace reaches megabytes;
/// past this size reading it would be work the agent chose.
const MAX_TRACE: u64 = 64 * 1024 * 1024;

/// Decides whether `trace` shows `agent`'s `SessionStart` hook succeeding. A success
/// settles it, whatever else is there; otherwise anything unreadable, or a trace
/// larger than any session leaves, makes it unknown.
pub(crate) fn session_start(trace: &Path, agent: &str) -> TraceEvidence {
    session_start_within(trace, agent, MAX_TRACE)
}

/// [`session_start`], reading at most `limit` bytes of the trace.
fn session_start_within(trace: &Path, agent: &str, limit: u64) -> TraceEvidence {
    match fs::symlink_metadata(trace) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= limit => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return TraceEvidence::Missing,
        // A link, a directory, a file too large, or one that cannot be inspected.
        _ => return TraceEvidence::Unknown,
    }
    read(trace, agent, limit).unwrap_or(TraceEvidence::Unknown)
}

fn read(trace: &Path, agent: &str, limit: u64) -> io::Result<TraceEvidence> {
    // Bounded again while reading, in case the file grew since it was inspected.
    let mut source = BufReader::new(File::open(trace)?.take(limit + 1));
    let mut line = Vec::new();
    let (mut failed, mut unknown) = (false, false);
    loop {
        match jsonl::next_line(&mut source, &mut line)? {
            Line::End if source.get_ref().limit() == 0 => return Ok(TraceEvidence::Unknown),
            Line::End => break,
            Line::TooLong => unknown = true,
            Line::Read if line.trim_ascii().is_empty() => {}
            Line::Read => match serde_json::from_slice::<Entry>(&line) {
                Ok(entry) if entry.event == "SessionStart" && entry.agent == agent => {
                    if entry.exit_code == 0 {
                        return Ok(TraceEvidence::Succeeded);
                    }
                    failed = true;
                }
                Ok(_) => {}
                Err(_) => unknown = true,
            },
        }
    }
    Ok(if unknown {
        TraceEvidence::Unknown
    } else if failed {
        TraceEvidence::Failed
    } else {
        TraceEvidence::Missing
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// A trace line as Symposium writes one, with its input and output.
    fn entry(event: &str, agent: &str, exit_code: u8) -> String {
        serde_json::json!({
            "event": event,
            "agent": agent,
            "input": { "hook_event_name": event, "cwd": "/trial/project" },
            "output": {},
            "exit_code": exit_code,
            "stderr": "",
        })
        .to_string()
    }

    /// The evidence for Claude in a trace holding `lines`.
    fn evidence(lines: &[String]) -> TraceEvidence {
        let dir = TempDir::new().unwrap();
        let trace = dir.path().join("hook-trace.jsonl");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&trace, text).unwrap();
        session_start(&trace, "claude")
    }

    #[test]
    fn a_successful_session_start_succeeded() {
        let lines = [
            entry("SessionStart", "claude", 0),
            entry("Stop", "claude", 0),
        ];

        assert_eq!(evidence(&lines), TraceEvidence::Succeeded);
    }

    #[test]
    fn no_trace_is_missing() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            session_start(&dir.path().join("hook-trace.jsonl"), "claude"),
            TraceEvidence::Missing
        );
    }

    #[test]
    fn a_trace_without_session_start_is_missing() {
        let lines = [
            entry("UserPromptSubmit", "claude", 0),
            entry("SessionStart", "codex", 0),
            String::new(),
        ];

        assert_eq!(evidence(&lines), TraceEvidence::Missing);
    }

    #[test]
    fn a_session_start_that_exited_otherwise_failed() {
        let lines = [entry("SessionStart", "claude", 2)];

        assert_eq!(evidence(&lines), TraceEvidence::Failed);
    }

    #[test]
    fn a_later_success_settles_it() {
        let lines = [
            entry("SessionStart", "claude", 1),
            "not json".to_owned(),
            entry("SessionStart", "claude", 0),
        ];

        assert_eq!(evidence(&lines), TraceEvidence::Succeeded);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_unknown_even_beside_a_failure() {
        for unreadable in [
            "not json".to_owned(),
            r#"{"event":"SessionStart","agent":"claude"}"#.to_owned(),
            r#"{"event":"SessionStart","agent":"claude","exit_code":"0"}"#.to_owned(),
            r#"{"event":"SessionStart","agent":"claude","exit_code":256}"#.to_owned(),
        ] {
            let lines = [entry("SessionStart", "claude", 1), unreadable.clone()];

            assert_eq!(evidence(&lines), TraceEvidence::Unknown, "{unreadable}");
        }
    }

    #[test]
    fn a_line_longer_than_any_read_is_unknown() {
        let long = format!(r#"{{"event":"x","pad":"{}"}}"#, "x".repeat(jsonl::MAX_LINE));

        assert_eq!(evidence(&[long]), TraceEvidence::Unknown);
    }

    /// A trace of `lines`, judged as if `limit` bytes were the most any session leaves.
    fn evidence_within(lines: &[String], limit: u64) -> TraceEvidence {
        let dir = TempDir::new().unwrap();
        let trace = dir.path().join("hook-trace.jsonl");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&trace, text).unwrap();
        session_start_within(&trace, "claude", limit)
    }

    #[test]
    fn many_short_lines_past_the_limit_are_unknown() {
        let mut lines = vec![entry("PostToolUse", "claude", 0); 100];
        lines.push(entry("SessionStart", "claude", 0));
        let size: usize = lines.iter().map(|line| line.len() + 1).sum();
        let size = size as u64;

        assert_eq!(evidence_within(&lines, size), TraceEvidence::Succeeded);
        assert_eq!(evidence_within(&lines, size - 1), TraceEvidence::Unknown);
    }

    #[test]
    fn a_trace_that_grows_past_the_limit_while_read_is_unknown() {
        let dir = TempDir::new().unwrap();
        let trace = dir.path().join("hook-trace.jsonl");
        let line = entry("PostToolUse", "claude", 0) + "\n";
        fs::write(&trace, line.repeat(10)).unwrap();

        // As if it had passed inspection at `limit` bytes and grown since. The reader
        // takes one byte past the limit, so this ends its reading on a whole line: only
        // the bound, not a cut-off line, can make the trace unknown.
        let limit = (line.len() * 5 - 1) as u64;
        assert_eq!(
            read(&trace, "claude", limit).unwrap(),
            TraceEvidence::Unknown
        );
    }

    #[test]
    fn a_directory_in_its_place_is_unknown() {
        let dir = TempDir::new().unwrap();
        let trace = dir.path().join("hook-trace.jsonl");
        fs::create_dir(&trace).unwrap();

        assert_eq!(session_start(&trace, "claude"), TraceEvidence::Unknown);
    }

    #[test]
    fn the_verdict_follows_from_the_evidence() {
        assert_eq!(TraceEvidence::Succeeded.verdict(), Verdict::Pass);
        assert_eq!(TraceEvidence::Failed.verdict(), Verdict::Fail);
        assert_eq!(TraceEvidence::Missing.verdict(), Verdict::Fail);
        assert_eq!(TraceEvidence::Unknown.verdict(), Verdict::Inconclusive);
    }

    /// Links need developer mode or elevation on Windows.
    #[cfg(unix)]
    #[test]
    fn a_linked_trace_is_unknown() {
        let dir = TempDir::new().unwrap();
        let forged = dir.path().join("forged.jsonl");
        fs::write(&forged, entry("SessionStart", "claude", 0) + "\n").unwrap();
        let trace = dir.path().join("hook-trace.jsonl");
        std::os::unix::fs::symlink(&forged, &trace).unwrap();

        assert_eq!(session_start(&trace, "claude"), TraceEvidence::Unknown);
    }

    /// The real hook's trace, read by this reader: the contract between them. Runs the
    /// `cargo-agents` built from this checkout in a trial under real `srt`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn the_session_start_hook_leaves_a_trace_this_reader_reads() {
        use crate::run::Configuration;
        use crate::run::client::Client;
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let trial = real.trial(1, Configuration::Symposium);
        real.set_up_symposium(&trial, Client::ClaudeCode);
        assert!(!trial.hook_trace().exists(), "a trace before the hook ran");

        let finished = real.run_session_start(&trial);

        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        assert_eq!(
            session_start(&trial.hook_trace(), Client::ClaudeCode.symposium_name()),
            TraceEvidence::Succeeded
        );
    }
}
