//! Whether Symposium's `SessionStart` hook ran and succeeded (RFD reference,
//! "Synchronization and hooks").
//!
//! Symposium appends a JSON line to `SYMPOSIUM_HOOK_TRACE` for each hook call that
//! reached a response, with its event, agent and exit code.

use std::path::Path;

use serde::Deserialize;

use super::log::{self, CallEvidence, Counted};

/// The fields this reader needs.
#[derive(Debug, Deserialize)]
struct Entry {
    event: String,
    agent: String,
    exit_code: u8,
}

/// Whether `trace` shows `agent`'s `SessionStart` hook succeeding.
pub(crate) fn session_start(trace: &Path, agent: &str) -> CallEvidence {
    log::calls(trace, Counted::Any, |entry: &Entry| {
        (entry.event == "SessionStart" && entry.agent == agent).then_some(entry.exit_code)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

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
    fn evidence(lines: &[String]) -> CallEvidence {
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

        assert_eq!(evidence(&lines), CallEvidence::Succeeded);
    }

    #[test]
    fn only_the_clients_session_start_counts() {
        let lines = [
            entry("UserPromptSubmit", "claude", 0),
            entry("SessionStart", "codex", 0),
        ];

        assert_eq!(evidence(&lines), CallEvidence::Missing);
    }

    #[test]
    fn a_session_start_that_exited_otherwise_failed() {
        let lines = [entry("SessionStart", "claude", 2)];

        assert_eq!(evidence(&lines), CallEvidence::Failed);
    }

    /// Runs the real hook, built from this checkout, under real `srt`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds symposium"]
    fn the_session_start_hook_leaves_a_trace_this_reader_reads() {
        use crate::run::Configuration;
        use crate::run::client::Client;
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_symposium};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_symposium());
        real.stage("symposium", &tool, Audience::Symposium);
        let trial = real.trial(1, Configuration::Symposium);
        real.set_up_symposium(&trial, Client::ClaudeCode);
        assert!(!trial.hook_trace().exists(), "a trace before the hook ran");

        let finished = real.run_session_start(&trial);

        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        assert_eq!(
            session_start(&trial.hook_trace(), Client::ClaudeCode.symposium_name()),
            CallEvidence::Succeeded
        );
    }
}
