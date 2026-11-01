//! Reading a Claude Code transcript (`stream-json`, one event per line) once, for its
//! ending, provenance and skill loads.
//!
//! Only `system/init` and `result` must have the expected shape; other events are
//! skipped, so a new event kind leaves the transcript readable. A message's payload is
//! decoded only for loads, so a reshaped message damages loads, never the ending.
//!
//! Reading continues past a bad line, so later evidence still counts. A mismatch
//! outranks a malformed line: another session's transcript says nothing about this one.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::value::RawValue;
use uuid::Uuid;

use super::calls::{Calls, Scope};
use super::loads::Skills;
use super::version::Version;
use crate::client::{
    Completion, InstalledSkill, Load, ProvenanceError, Reading, TranscriptError, Unusable,
};

/// What every event is first read for.
#[derive(Debug, Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: String,
}

/// A `system` event, read for its `subtype`, which not every system event has.
#[derive(Debug, Deserialize)]
struct SystemHead {
    subtype: Option<String>,
}

/// An assistant or user message. Its payload is left undecoded here, borrowed from the
/// line, since only the load evidence reads it.
#[derive(Debug, Deserialize)]
struct Envelope<'a> {
    session_id: Option<String>,
    #[serde(borrow)]
    message: &'a RawValue,
}

/// A `system/init` event: who the client says it is.
#[derive(Debug, Deserialize)]
struct Init {
    session_id: String,
    claude_code_version: String,
    /// Where Claude Code ran, which a relative tool path is resolved against.
    cwd: Option<PathBuf>,
}

/// A `result` event.
#[derive(Debug, Deserialize)]
struct Ending {
    session_id: String,
    subtype: String,
    is_error: bool,
    /// The final message, or an API error in Claude Code's words.
    result: Option<String>,
    /// What went wrong, on the result an error ends with, which has no `result`.
    #[serde(default)]
    errors: Vec<String>,
}

/// The state of reading one transcript.
struct Reader<'a> {
    session: String,
    version: Version,
    completion: Completion,
    /// Whether an `init` has been seen. One that cannot be read counts: the transcript
    /// is then malformed, not unattributed.
    introduced: bool,
    /// Why the transcript cannot say how the session ended, if it cannot.
    unusable: Option<Unusable>,
    calls: Calls<Skills<'a>>,
}

/// Reads the transcript at `path`, of `session` run by Claude Code `version` in
/// `project`, for its completion and how it shows each of `skills` reaching the agent.
pub(super) fn read(
    path: &Path,
    project: &Path,
    session: Uuid,
    version: Version,
    skills: &[InstalledSkill],
) -> Reading {
    let mut reader = Reader::new(project, session, version, skills);
    match File::open(path) {
        Ok(file) => reader.lines(BufReader::new(file), path),
        Err(source) => reader.unreadable(TranscriptError::Read {
            path: path.to_owned(),
            source,
        }),
    }
    reader.reading()
}

/// [`read`], for a transcript held in memory.
#[cfg(test)]
pub(super) fn read_text(
    text: &str,
    project: &Path,
    session: Uuid,
    version: Version,
    skills: &[InstalledSkill],
) -> Reading {
    let mut reader = Reader::new(project, session, version, skills);
    reader.lines(text.as_bytes(), Path::new("transcript.jsonl"));
    reader.reading()
}

impl<'a> Reader<'a> {
    fn new(
        project: &'a Path,
        session: Uuid,
        version: Version,
        skills: &'a [InstalledSkill],
    ) -> Self {
        Self {
            session: session.to_string(),
            version,
            completion: Completion::Missing,
            introduced: false,
            unusable: None,
            calls: Calls::new(Skills::new(skills, project)),
        }
    }

    /// Reads `transcript`, found at `path`, a line at a time, since tool results can make
    /// it large.
    fn lines(&mut self, mut transcript: impl BufRead, path: &Path) {
        let mut line = Vec::new();
        for number in 1.. {
            line.clear();
            match transcript.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(source) => {
                    let path = path.to_owned();
                    return self.unreadable(TranscriptError::Read { path, source });
                }
            }
            if line.trim_ascii().is_empty() {
                continue;
            }
            if let Err(error) = self.line(&line, number) {
                self.unreadable(error);
            }
        }
    }

    /// Reads one event. An error is a line the ending may rest on that cannot be read.
    fn line(&mut self, line: &[u8], number: usize) -> Result<(), TranscriptError> {
        let head: Head = parse(line, number)?;
        match head.kind.as_str() {
            "system" => {
                let system: SystemHead = parse(line, number)?;
                if system.subtype.as_deref() == Some("init") {
                    self.introduced = true;
                    self.init(parse(line, number)?);
                }
            }
            "result" => self.ending(parse(line, number)?),
            "assistant" | "user" => match serde_json::from_slice::<Envelope<'_>>(line) {
                Ok(envelope) => self.message(&head.kind, envelope),
                Err(_) => self.calls.damage(),
            },
            _ => {}
        }
        Ok(())
    }

    fn init(&mut self, init: Init) {
        if let Some(cwd) = &init.cwd {
            self.calls.matcher_mut().ran_in(cwd);
        }
        if let Err(error) = init.check(&self.session, self.version) {
            self.mismatched(error);
        }
    }

    fn ending(&mut self, ending: Ending) {
        if !self.introduced {
            self.mismatched(ProvenanceError::Unstated);
        } else if self.is_ours(&ending.session_id) {
            self.completion = ending.completion();
            self.calls.finish();
        }
    }

    /// Hands a message of `kind` to the tool call evidence, if it is this session's.
    fn message(&mut self, kind: &str, envelope: Envelope<'_>) {
        match (envelope.session_id, kind) {
            (Some(session_id), _) => {
                if self.is_ours(&session_id) {
                    self.calls.message(envelope.message, Scope::Session);
                }
            }
            (None, "user") => self.calls.message(envelope.message, Scope::Unscoped),
            // Nothing says whose calls these are.
            (None, _) => self.calls.damage(),
        }
    }

    /// Whether an event of `session_id` is this session's; another session's is a
    /// mismatch.
    fn is_ours(&mut self, session_id: &str) -> bool {
        let ours = session_id == self.session;
        if !ours {
            self.mismatched(ProvenanceError::Session {
                requested: self.session.clone(),
                found: session_id.to_owned(),
            });
        }
        ours
    }

    /// The transcript is not this session's: it decides over any malformed line.
    fn mismatched(&mut self, error: ProvenanceError) {
        if !matches!(self.unusable, Some(Unusable::Provenance(_))) {
            self.unusable = Some(error.into());
        }
        self.calls.damage();
    }

    /// Part of the transcript cannot be read; the first such problem is kept.
    fn unreadable(&mut self, error: TranscriptError) {
        self.unusable.get_or_insert(error.into());
        self.calls.damage();
    }

    fn reading(self) -> Reading {
        Reading {
            completion: self.unusable.map_or(Ok(self.completion), Err),
            loads: self.calls.outcomes().into_iter().map(Load::from).collect(),
        }
    }
}

/// Reads `line`, the transcript's line `number`, as a `T`.
fn parse<'a, T: Deserialize<'a>>(line: &'a [u8], number: usize) -> Result<T, TranscriptError> {
    serde_json::from_slice(line).map_err(|error| TranscriptError::Malformed {
        line: number,
        reason: error.to_string(),
    })
}

impl Init {
    fn check(&self, session: &str, version: Version) -> Result<(), ProvenanceError> {
        if self.session_id != session {
            return Err(ProvenanceError::Session {
                requested: session.to_owned(),
                found: self.session_id.clone(),
            });
        }
        // A version that does not parse is not the one checked, so it is a mismatch too.
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
    /// Only `is_error` decides: an API error arrives with `subtype` `success`.
    fn completion(self) -> Completion {
        if !self.is_error {
            return Completion::Finished;
        }
        // Claude Code's own words say more than the subtype, which some failures give
        // alone.
        let message = match self.result {
            Some(result) if !result.is_empty() => result,
            _ if !self.errors.is_empty() => self.errors.join("; "),
            _ => self.subtype,
        };
        Completion::Reported(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::claude::fixtures::{
        FINISHED, MODEL_ERROR, SKILL_LOADED, as_session, before, edited, first, recorded_session,
        reshaped, without, without_user_sessions,
    };
    use crate::client::claude::version;

    fn completion_as(
        transcript: &str,
        session: Uuid,
        version: Version,
    ) -> Result<Completion, Unusable> {
        read_text(
            transcript,
            Path::new("/trial/project"),
            session,
            version,
            &[],
        )
        .completion
    }

    /// As the recorded session, run by the version it was recorded with.
    fn completion(transcript: &str) -> Result<Completion, Unusable> {
        completion_as(transcript, recorded_session(), version::MINIMUM)
    }

    fn recorded_init() -> &'static str {
        first(FINISHED, "system/init")
    }

    fn recorded_result() -> &'static str {
        first(FINISHED, "result")
    }

    /// `event` with its `field` removed.
    fn without_field(event: &str, field: &str) -> String {
        edited(event, |event| {
            event.remove(field).unwrap();
        })
    }

    fn without_result(transcript: &str) -> String {
        without(transcript, "result")
    }

    /// The line number a line added after `transcript` has.
    fn next_line(transcript: &str) -> usize {
        transcript.lines().count() + 1
    }

    fn malformed_line(completion: Result<Completion, Unusable>) -> usize {
        match completion {
            Err(Unusable::Transcript(TranscriptError::Malformed { line, .. })) => line,
            other => panic!("expected a malformed transcript, got {other:?}"),
        }
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
        let result = edited(recorded_result(), |event| {
            event.insert("subtype".to_owned(), "error_max_turns".into());
            event.insert("is_error".to_owned(), true.into());
            event.remove("result").unwrap();
        });
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(
            completion(&transcript).unwrap(),
            Completion::Reported("error_max_turns".to_owned())
        );
    }

    /// The recorded result, ended with `subtype` and `errors`, without its `result`.
    fn error_result(subtype: &str, errors: &[&str]) -> String {
        edited(recorded_result(), |result| {
            result.remove("result");
            result.insert("is_error".to_owned(), true.into());
            result.insert("subtype".to_owned(), subtype.into());
            result.insert("errors".to_owned(), errors.into());
        })
    }

    #[test]
    fn an_error_is_reported_by_its_errors() {
        let result = error_result("error_during_execution", &["tool crashed", "gave up"]);
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(
            completion(&transcript).unwrap(),
            Completion::Reported("tool crashed; gave up".to_owned())
        );
    }

    #[test]
    fn an_empty_result_gives_way_to_the_errors() {
        let result = edited(
            &error_result("error_max_budget_usd", &["over budget"]),
            |result| {
                result.insert("result".to_owned(), "".into());
            },
        );
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(
            completion(&transcript).unwrap(),
            Completion::Reported("over budget".to_owned())
        );
    }

    #[test]
    fn an_error_with_no_words_is_reported_by_its_subtype() {
        let result = error_result("error_during_execution", &[]);
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(
            completion(&transcript).unwrap(),
            Completion::Reported("error_during_execution".to_owned())
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
        let complete = without_result(FINISHED);
        let transcript = format!("{complete}not json\n");

        assert_eq!(
            malformed_line(completion(&transcript)),
            next_line(&complete)
        );
    }

    #[test]
    fn a_last_line_cut_off_mid_write_is_malformed() {
        let transcript = format!("{FINISHED}{{\"type\":\"assist");

        assert_eq!(malformed_line(completion(&transcript)), next_line(FINISHED));
    }

    #[test]
    fn a_blank_line_is_skipped() {
        let transcript = FINISHED.replacen('\n', "\n\n  \n", 1);

        assert_eq!(completion(&transcript).unwrap(), Completion::Finished);
    }

    #[test]
    fn a_result_without_is_error_is_malformed() {
        let transcript = r#"{"type":"result","subtype":"success","result":"ok"}"#;

        assert_eq!(malformed_line(completion(transcript)), 1);
    }

    #[test]
    fn the_first_malformed_line_is_the_one_reported() {
        let complete = without_result(FINISHED);
        let transcript = format!("{complete}not json\nnor this\n");

        assert_eq!(
            malformed_line(completion(&transcript)),
            next_line(&complete)
        );
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
    fn a_mismatch_outranks_an_earlier_malformed_line() {
        let transcript = format!("not json\n{FINISHED}");

        let error = completion_as(&transcript, Uuid::new_v4(), version::MINIMUM).unwrap_err();

        assert!(
            matches!(error, Unusable::Provenance(ProvenanceError::Session { .. })),
            "{error:?}"
        );
    }

    #[test]
    fn a_result_without_an_init_is_unstated() {
        let transcript = without(FINISHED, "system/init");

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
            as_session(recorded_result(), other)
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
            as_session(recorded_result(), Uuid::new_v4()),
            recorded_result()
        );

        assert!(matches!(
            completion(&transcript),
            Err(Unusable::Provenance(ProvenanceError::Session { .. }))
        ));
    }

    #[test]
    fn another_sessions_message_is_mismatched() {
        let assistant = first(FINISHED, "assistant");
        let transcript = format!(
            "{}{}\n{}\n",
            without_result(FINISHED),
            as_session(assistant, Uuid::new_v4()),
            recorded_result()
        );

        assert!(matches!(
            completion(&transcript),
            Err(Unusable::Provenance(ProvenanceError::Session { .. }))
        ));
    }

    #[test]
    fn a_user_message_without_a_session_still_completes() {
        let transcript = without_user_sessions(SKILL_LOADED);

        assert_eq!(completion(&transcript).unwrap(), Completion::Finished);
    }

    #[test]
    fn a_message_claude_code_reshaped_leaves_the_ending_alone() {
        let transcript = reshaped(FINISHED, "assistant");

        assert_eq!(completion(&transcript).unwrap(), Completion::Finished);
    }

    #[test]
    fn a_system_event_without_a_subtype_is_skipped() {
        let transcript = format!("{{\"type\":\"system\"}}\n{FINISHED}");

        assert_eq!(completion(&transcript).unwrap(), Completion::Finished);
    }

    #[test]
    fn an_init_without_its_version_is_malformed() {
        let init = without_field(recorded_init(), "claude_code_version");
        let transcript = format!("{init}\n{}\n", recorded_result());

        assert_eq!(malformed_line(completion(&transcript)), 1);
    }

    #[test]
    fn a_result_without_a_session_is_malformed() {
        let result = without_field(recorded_result(), "session_id");
        let transcript = format!("{}\n{result}\n", recorded_init());

        assert_eq!(malformed_line(completion(&transcript)), 2);
    }

    #[test]
    fn a_client_that_stops_before_init_has_no_completion() {
        let hook = r#"{"type":"system","subtype":"hook_started","hook_event":"SessionStart","session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let transcript = format!("{}{hook}\n", before(FINISHED, "system/init"));

        assert_eq!(completion(&transcript).unwrap(), Completion::Missing);
    }

    #[test]
    fn a_missing_transcript_is_unreadable() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("claude.stdout.log");

        let reading = read(
            &missing,
            Path::new("/trial/project"),
            recorded_session(),
            version::MINIMUM,
            &[],
        );

        assert!(matches!(
            reading.completion,
            Err(Unusable::Transcript(TranscriptError::Read { .. }))
        ));
    }
}
