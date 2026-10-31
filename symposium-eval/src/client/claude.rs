//! The Claude Code adapter (RFD reference, "Running the client").
//!
//! The transcript is `--output-format stream-json`: one JSON event per line. The last
//! `result` event says how the session ended. Claude Code marks an API error with
//! `is_error` even when the event's `subtype` is `success`, so only `is_error` is read.
//! Events of other types are skipped, so a new kind of event leaves the transcript
//! readable.
//!
//! The adapter builds the whole command line: several valid flags, such as `--bare` or
//! `--no-session-persistence`, would skip hooks and skills or drop the session's own
//! file, and so invalidate a trial.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;
use uuid::Uuid;

use super::{ClientRunner, Completion, TranscriptError};
use crate::grade::process::Invocation;
use crate::run::client::Client;
use crate::run::{Machine, RunError, Trial};

/// One Claude Code session, running one model.
#[derive(Debug, Clone)]
pub(crate) struct Claude {
    model: String,
    /// Names the session's own transcript under the trial's home, which survives a
    /// timeout, and lets the runner check that the stream is this session's.
    session: Uuid,
    /// The arguments that set how Claude Code confines the agent.
    sandbox: Vec<OsString>,
}

impl Claude {
    /// For an enforced trial: `srt` is the boundary, so Claude Code asks no permission.
    pub(crate) fn enforced(model: String) -> Self {
        Self::new(
            model,
            vec!["--permission-mode".into(), "bypassPermissions".into()],
        )
    }

    /// For an exploratory trial, confined by Claude Code's own sandbox, after writing the
    /// trial's settings file.
    ///
    /// # Errors
    ///
    /// As [`Trial::exploratory_arguments`].
    pub(crate) fn exploratory(
        model: String,
        trial: &Trial,
        machine: &Machine,
    ) -> Result<Self, RunError> {
        let sandbox = trial.exploratory_arguments(Client::ClaudeCode, machine)?;
        Ok(Self::new(model, sandbox))
    }

    fn new(model: String, sandbox: Vec<OsString>) -> Self {
        Self {
            model,
            session: Uuid::new_v4(),
            sandbox,
        }
    }

    pub(crate) fn session(&self) -> Uuid {
        self.session
    }
}

impl ClientRunner for Claude {
    fn invocation(
        &self,
        trial: &Trial,
        prompt: &str,
        env: Vec<(OsString, OsString)>,
    ) -> Invocation {
        let mut args: Vec<OsString> = vec![
            "-p".into(),
            "--output-format".into(),
            "stream-json".into(),
            // Print mode refuses `stream-json` without it.
            "--verbose".into(),
            // Without it, hooks other than `SessionStart`, such as `Stop`, are left out.
            "--include-hook-events".into(),
            "--session-id".into(),
            self.session.to_string().into(),
            "--model".into(),
            self.model.clone().into(),
        ];
        args.extend(self.sandbox.iter().cloned());
        // So a prompt that starts with `-` is not read as an option.
        args.extend(["--".into(), prompt.into()]);
        Invocation {
            program: trial.bin().join("claude").into(),
            args,
            dir: trial.project(),
            env,
            log_name: "claude".to_owned(),
        }
    }

    fn completion(&self, transcript: &Path) -> Result<Completion, TranscriptError> {
        let file = File::open(transcript).map_err(|source| TranscriptError::Read {
            path: transcript.to_owned(),
            source,
        })?;
        completion(BufReader::new(file), transcript)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Result(Ending),
    #[serde(other)]
    Other,
}

/// A `result` event.
#[derive(Debug, Deserialize)]
struct Ending {
    subtype: String,
    is_error: bool,
    /// The final message, or the error in Claude Code's words; absent for some errors.
    result: Option<String>,
}

/// Reads `transcript`, found at `path`, a line at a time, since tool results can make
/// it large.
fn completion(mut transcript: impl BufRead, path: &Path) -> Result<Completion, TranscriptError> {
    let mut completion = Completion::Missing;
    let mut line = Vec::new();
    for number in 1.. {
        line.clear();
        let read =
            transcript
                .read_until(b'\n', &mut line)
                .map_err(|source| TranscriptError::Read {
                    path: path.to_owned(),
                    source,
                })?;
        if read == 0 {
            break;
        }
        let event: Event =
            serde_json::from_slice(&line).map_err(|error| TranscriptError::Malformed {
                line: number,
                reason: error.to_string(),
            })?;
        if let Event::Result(ending) = event {
            completion = ending.completion();
        }
    }
    Ok(completion)
}

impl Ending {
    fn completion(self) -> Completion {
        if self.is_error {
            Completion::Reported(self.result.unwrap_or(self.subtype))
        } else {
            Completion::Finished
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Configuration;
    use crate::run::test_support::{Host, trial};

    /// Recorded from Claude Code 2.1.316, with personal details removed.
    const FINISHED: &str = include_str!("claude/transcripts/finished.jsonl");
    /// The same version, run with a model that does not exist.
    const MODEL_ERROR: &str = include_str!("claude/transcripts/model_error.jsonl");

    /// Valid flags that would invalidate a trial (research notes, Step 4): they skip hooks
    /// or skills, ignore the trial's settings, drop the session's file, or add partial
    /// events the parser does not read.
    const FORBIDDEN: [&str; 7] = [
        "--bare",
        "--safe-mode",
        "--disable-slash-commands",
        "--restricted",
        "--setting-sources",
        "--no-session-persistence",
        "--include-partial-messages",
    ];

    fn completion(transcript: &str) -> Result<Completion, TranscriptError> {
        super::completion(transcript.as_bytes(), Path::new("transcript.jsonl"))
    }

    fn without_result(transcript: &str) -> String {
        transcript
            .lines()
            .filter(|line| !line.starts_with(r#"{"type":"result""#))
            .map(|line| format!("{line}\n"))
            .collect()
    }

    fn malformed_line(completion: Result<Completion, TranscriptError>) -> usize {
        match completion {
            Err(TranscriptError::Malformed { line, .. }) => line,
            other => panic!("expected a malformed transcript, got {other:?}"),
        }
    }

    fn model() -> String {
        "example-model".to_owned()
    }

    #[test]
    fn a_recorded_session_finishes() {
        assert_eq!(completion(FINISHED).unwrap(), Completion::Finished);
    }

    #[test]
    fn a_recorded_api_error_is_reported_in_claudes_words() {
        let Completion::Reported(message) = completion(MODEL_ERROR).unwrap() else {
            panic!("expected a reported error");
        };
        assert!(message.contains("not-a-model"), "{message}");
    }

    #[test]
    fn an_error_without_a_message_is_reported_by_its_subtype() {
        let transcript = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;

        assert_eq!(
            completion(transcript).unwrap(),
            Completion::Reported("error_max_turns".to_owned())
        );
    }

    #[test]
    fn a_transcript_without_a_result_has_no_completion() {
        assert_eq!(
            completion(&without_result(FINISHED)).unwrap(),
            Completion::Missing
        );
    }

    #[test]
    fn an_unknown_event_is_skipped() {
        let transcript = format!("{{\"type\":\"new_kind\",\"field\":1}}\n{FINISHED}");

        assert_eq!(completion(&transcript).unwrap(), Completion::Finished);
    }

    #[test]
    fn a_line_that_is_not_an_event_is_malformed() {
        let transcript = format!("{}not json\n", without_result(FINISHED));

        assert_eq!(malformed_line(completion(&transcript)), 6);
    }

    #[test]
    fn a_result_without_is_error_is_malformed() {
        let transcript = r#"{"type":"result","subtype":"success","result":"ok"}"#;

        assert_eq!(malformed_line(completion(transcript)), 1);
    }

    #[test]
    fn an_enforced_session_asks_no_permission_and_ends_with_the_prompt() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let claude = Claude::enforced(model());

        let invocation = claude.invocation(&trial, "-do the task", Vec::new());

        assert_eq!(invocation.program, trial.bin().join("claude"));
        assert_eq!(invocation.dir, trial.project());
        assert_eq!(
            invocation.args,
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-hook-events",
                "--session-id",
                &claude.session().to_string(),
                "--model",
                "example-model",
                "--permission-mode",
                "bypassPermissions",
                "--",
                "-do the task",
            ]
        );
    }

    #[test]
    fn an_exploratory_session_uses_the_trials_own_sandbox() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let claude = Claude::exploratory(model(), &trial, &host.machine()).unwrap();

        let invocation = claude.invocation(&trial, "do the task", Vec::new());

        let sandbox = trial
            .exploratory_arguments(Client::ClaudeCode, &host.machine())
            .unwrap();
        assert_eq!(invocation.args[9..invocation.args.len() - 2], sandbox);
    }

    #[test]
    fn each_session_has_its_own_id() {
        assert_ne!(
            Claude::enforced(model()).session(),
            Claude::enforced(model()).session()
        );
    }

    #[test]
    fn no_session_passes_a_flag_that_invalidates_the_trial() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let sessions = [
            Claude::enforced(model()),
            Claude::exploratory(model(), &trial, &host.machine()).unwrap(),
        ];

        for claude in sessions {
            let invocation = claude.invocation(&trial, "do the task", Vec::new());
            let options = &invocation.args[..invocation.args.len() - 1];
            for flag in FORBIDDEN {
                assert!(
                    options.iter().all(|arg| arg != flag),
                    "{flag} in {options:?}"
                );
            }
        }
    }
}
