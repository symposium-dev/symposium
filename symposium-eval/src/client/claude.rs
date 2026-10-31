//! The Claude Code adapter (RFD reference, "Running the client").
//!
//! The transcript is `--output-format stream-json`: one JSON event per line. The last
//! `result` event says how the session ended. Claude Code marks an API error with
//! `is_error` even when the event's `subtype` is `success`, so only `is_error` is read.
//! Events of other types are skipped, so a new kind of event leaves the transcript
//! readable.
//!
//! Every `system/init` event must name the session the runner started and the version
//! preflight checked (research notes, Step 4, client provenance), and every `result`
//! that session. A `result` before any `init` cannot say whose it is. A transcript that
//! ends before `init` is only a client that stopped early.
//!
//! The adapter builds the whole command line: several valid flags, such as `--bare` or
//! `--no-session-persistence`, would skip hooks and skills or drop the session's own
//! file, and so invalidate a trial.

pub(crate) mod version;

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;
use uuid::Uuid;

use self::version::Version;
use super::{ClientRunner, Completion, ProvenanceError, TranscriptError, Unusable};
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
    /// What preflight found the staged client to be.
    version: Version,
    /// The arguments that set how Claude Code confines the agent.
    sandbox: Vec<OsString>,
}

impl Claude {
    /// For an enforced trial, of the `version` preflight found: `srt` is the boundary,
    /// so Claude Code asks no permission.
    pub(crate) fn enforced(model: String, version: Version) -> Self {
        Self::new(
            model,
            version,
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
        version: Version,
        trial: &Trial,
        machine: &Machine,
    ) -> Result<Self, RunError> {
        let sandbox = trial.exploratory_arguments(Client::ClaudeCode, machine)?;
        Ok(Self::new(model, version, sandbox))
    }

    fn new(model: String, version: Version, sandbox: Vec<OsString>) -> Self {
        Self {
            model,
            session: Uuid::new_v4(),
            version,
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

    fn completion(&self, transcript: &Path) -> Result<Completion, Unusable> {
        let file = File::open(transcript).map_err(|source| TranscriptError::Read {
            path: transcript.to_owned(),
            source,
        })?;
        completion(BufReader::new(file), transcript, self.session, self.version)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    System(System),
    Result(Ending),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
enum System {
    Init(Init),
    #[serde(other)]
    Other,
}

/// A `system/init` event: who the client says it is.
#[derive(Debug, Deserialize)]
struct Init {
    session_id: String,
    claude_code_version: String,
}

/// A `result` event.
#[derive(Debug, Deserialize)]
struct Ending {
    session_id: String,
    subtype: String,
    is_error: bool,
    /// The final message, or the error in Claude Code's words; absent for some errors.
    result: Option<String>,
}

/// Reads `transcript`, found at `path`, a line at a time, since tool results can make
/// it large, checking that it is `session`, run by Claude Code `version`.
fn completion(
    mut transcript: impl BufRead,
    path: &Path,
    session: Uuid,
    version: Version,
) -> Result<Completion, Unusable> {
    let mut completion = Completion::Missing;
    let mut introduced = false;
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
        match event {
            Event::System(System::Init(init)) => {
                init.check(session, version)?;
                introduced = true;
            }
            Event::Result(ending) => {
                if !introduced {
                    return Err(ProvenanceError::Unstated.into());
                }
                is_session(&ending.session_id, session)?;
                completion = ending.completion();
            }
            Event::System(System::Other) | Event::Other => {}
        }
    }
    Ok(completion)
}

/// Whether `found`, an event's `session_id`, is `session`.
fn is_session(found: &str, session: Uuid) -> Result<(), ProvenanceError> {
    let requested = session.to_string();
    if found == requested {
        Ok(())
    } else {
        Err(ProvenanceError::Session {
            requested,
            found: found.to_owned(),
        })
    }
}

impl Init {
    fn check(&self, session: Uuid, version: Version) -> Result<(), ProvenanceError> {
        is_session(&self.session_id, session)?;
        if Version::from_output(&self.claude_code_version) != Some(version) {
            return Err(ProvenanceError::Version {
                checked: version.to_string(),
                found: self.claude_code_version.clone(),
            });
        }
        Ok(())
    }
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

    /// The session the recorded transcripts were rewritten to.
    fn recorded_session() -> Uuid {
        Uuid::parse_str("00000000-0000-4000-8000-000000000000").unwrap()
    }

    fn completion_as(
        transcript: &str,
        session: Uuid,
        version: Version,
    ) -> Result<Completion, Unusable> {
        super::completion(
            transcript.as_bytes(),
            Path::new("transcript.jsonl"),
            session,
            version,
        )
    }

    /// As the recorded session, run by the version it was recorded with.
    fn completion(transcript: &str) -> Result<Completion, Unusable> {
        completion_as(transcript, recorded_session(), version::MINIMUM)
    }

    fn recorded_init() -> &'static str {
        FINISHED
            .lines()
            .find(|line| line.contains(r#""subtype":"init""#))
            .unwrap()
    }

    fn recorded_result() -> &'static str {
        FINISHED
            .lines()
            .find(|line| line.starts_with(r#"{"type":"result""#))
            .unwrap()
    }

    /// `event` with its `field` removed.
    fn without_field(event: &str, field: &str) -> String {
        let mut event: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(event).unwrap();
        event.remove(field).unwrap();
        serde_json::to_string(&event).unwrap()
    }

    /// `event` as if session `other` had written it.
    fn from_session(event: &str, other: Uuid) -> String {
        event.replace(&recorded_session().to_string(), &other.to_string())
    }

    fn without(transcript: &str, prefix: &str) -> String {
        transcript
            .lines()
            .filter(|line| !line.starts_with(prefix))
            .map(|line| format!("{line}\n"))
            .collect()
    }

    fn without_result(transcript: &str) -> String {
        without(transcript, r#"{"type":"result""#)
    }

    fn malformed_line(completion: Result<Completion, Unusable>) -> usize {
        match completion {
            Err(Unusable::Transcript(TranscriptError::Malformed { line, .. })) => line,
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
        let result = recorded_result().replace(
            r#""subtype":"success","is_error":false"#,
            r#""subtype":"error_max_turns","is_error":true"#,
        );
        let result = without_field(&result, "result");
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(
            completion(&transcript).unwrap(),
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
    fn another_sessions_transcript_is_mismatched() {
        let error = completion_as(FINISHED, Uuid::new_v4(), version::MINIMUM).unwrap_err();

        assert!(
            matches!(&error, Unusable::Provenance(ProvenanceError::Session { found, .. }) if *found == recorded_session().to_string()),
            "{error:?}"
        );
    }

    #[test]
    fn a_transcript_from_another_client_version_is_mismatched() {
        let checked = Version::from_output("2.1.400").unwrap();

        let error = completion_as(FINISHED, recorded_session(), checked).unwrap_err();

        assert!(
            matches!(&error, Unusable::Provenance(ProvenanceError::Version { found, .. }) if found == "2.1.316"),
            "{error:?}"
        );
    }

    #[test]
    fn a_result_without_an_init_is_unstated() {
        let transcript = without(FINISHED, r#"{"type":"system","subtype":"init""#);

        assert!(matches!(
            completion(&transcript),
            Err(Unusable::Provenance(ProvenanceError::Unstated))
        ));
    }

    #[test]
    fn a_result_before_the_init_is_unstated() {
        let transcript = format!("{}\n{FINISHED}", recorded_result());

        assert!(matches!(
            completion(&transcript),
            Err(Unusable::Provenance(ProvenanceError::Unstated))
        ));
    }

    #[test]
    fn another_sessions_result_after_this_sessions_init_is_mismatched() {
        let other = Uuid::new_v4();
        let transcript = format!(
            "{}{}\n",
            without_result(FINISHED),
            from_session(recorded_result(), other)
        );

        let error = completion(&transcript).unwrap_err();

        assert!(
            matches!(&error, Unusable::Provenance(ProvenanceError::Session { found, .. }) if *found == other.to_string()),
            "{error:?}"
        );
    }

    #[test]
    fn one_foreign_result_among_several_is_mismatched() {
        let transcript = format!(
            "{FINISHED}{}\n{}\n",
            from_session(recorded_result(), Uuid::new_v4()),
            recorded_result()
        );

        assert!(matches!(
            completion(&transcript),
            Err(Unusable::Provenance(ProvenanceError::Session { .. }))
        ));
    }

    #[test]
    fn a_result_without_a_session_is_malformed() {
        let result = without_field(recorded_result(), "session_id");
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(malformed_line(completion(&transcript)), 2);
    }

    #[test]
    fn a_client_that_stops_before_init_has_no_completion() {
        let hooks: String = FINISHED
            .lines()
            .take(2)
            .map(|line| format!("{line}\n"))
            .collect();

        assert_eq!(completion(&hooks).unwrap(), Completion::Missing);
    }

    #[test]
    fn an_enforced_session_asks_no_permission_and_ends_with_the_prompt() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let claude = Claude::enforced(model(), version::MINIMUM);

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
        let claude =
            Claude::exploratory(model(), version::MINIMUM, &trial, &host.machine()).unwrap();

        let invocation = claude.invocation(&trial, "do the task", Vec::new());

        let sandbox = trial
            .exploratory_arguments(Client::ClaudeCode, &host.machine())
            .unwrap();
        assert_eq!(invocation.args[9..invocation.args.len() - 2], sandbox);
    }

    #[test]
    fn each_session_has_its_own_id() {
        assert_ne!(
            Claude::enforced(model(), version::MINIMUM).session(),
            Claude::enforced(model(), version::MINIMUM).session()
        );
    }

    #[test]
    fn no_session_passes_a_flag_that_invalidates_the_trial() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let sessions = [
            Claude::enforced(model(), version::MINIMUM),
            Claude::exploratory(model(), version::MINIMUM, &trial, &host.machine()).unwrap(),
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
