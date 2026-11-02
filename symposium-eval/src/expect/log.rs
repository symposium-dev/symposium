//! Logs written inside the trial: one JSON line per call, with its exit code.
//!
//! The agent can write them too, so a log is read only as a regular file, with no
//! links followed and its size bounded.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::Path;

use serde::de::DeserializeOwned;

use super::Verdict;
use crate::jsonl::{self, Line};

/// Larger than any log a session leaves; a hook trace can reach megabytes.
pub(crate) const MAX_LOG: u64 = 64 * 1024 * 1024;

/// What a log showed about the calls asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(crate) enum CallEvidence {
    /// A call that exited 0.
    Succeeded,
    /// Only calls that exited otherwise.
    Failed,
    /// No call.
    Missing,
    /// No successful call, and something that could not be read.
    Unknown,
}

impl CallEvidence {
    pub(crate) fn verdict(self) -> Verdict {
        match self {
            Self::Succeeded => Verdict::Pass,
            Self::Failed | Self::Missing => Verdict::Fail,
            Self::Unknown => Verdict::Inconclusive,
        }
    }
}

/// Which calls decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Counted {
    /// Any successful call.
    Any,
    /// The first call, as the RFD asks for a `Stop` hook.
    First,
}

/// What `log` shows of the calls `exit_of` picks out, by their exit codes.
pub(crate) fn calls<T: DeserializeOwned>(
    log: &Path,
    counted: Counted,
    exit_of: impl Fn(&T) -> Option<u8>,
) -> CallEvidence {
    calls_within(log, MAX_LOG, counted, exit_of)
}

/// [`calls`], reading at most `limit` bytes of the log.
pub(crate) fn calls_within<T: DeserializeOwned>(
    log: &Path,
    limit: u64,
    counted: Counted,
    exit_of: impl Fn(&T) -> Option<u8>,
) -> CallEvidence {
    match fs::symlink_metadata(log) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= limit => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return CallEvidence::Missing,
        _ => return CallEvidence::Unknown,
    }
    read(log, limit, counted, exit_of).unwrap_or(CallEvidence::Unknown)
}

fn read<T: DeserializeOwned>(
    log: &Path,
    limit: u64,
    counted: Counted,
    exit_of: impl Fn(&T) -> Option<u8>,
) -> io::Result<CallEvidence> {
    let first = counted == Counted::First;
    // Bounded again, in case the file grew.
    let mut source = BufReader::new(File::open(log)?.take(limit + 1));
    let mut line = Vec::new();
    let (mut failed, mut unknown) = (false, false);
    loop {
        match jsonl::next_line(&mut source, &mut line)? {
            Line::End if source.get_ref().limit() == 0 => return Ok(CallEvidence::Unknown),
            Line::End => break,
            // A line that cannot be read could be the first call.
            Line::TooLong if first => return Ok(CallEvidence::Unknown),
            Line::TooLong => unknown = true,
            Line::Read if line.trim_ascii().is_empty() => {}
            Line::Read => match serde_json::from_slice::<T>(&line).map(|entry| exit_of(&entry)) {
                Ok(Some(0)) => return Ok(CallEvidence::Succeeded),
                Ok(Some(_)) if first => return Ok(CallEvidence::Failed),
                Ok(Some(_)) => failed = true,
                Ok(None) => {}
                Err(_) if first => return Ok(CallEvidence::Unknown),
                Err(_) => unknown = true,
            },
        }
    }
    Ok(if unknown {
        CallEvidence::Unknown
    } else if failed {
        CallEvidence::Failed
    } else {
        CallEvidence::Missing
    })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use tempfile::TempDir;

    use super::*;

    /// A call as a log records it.
    #[derive(Debug, Deserialize)]
    struct Call {
        name: String,
        exit_code: u8,
    }

    fn call(name: &str, exit_code: u8) -> String {
        serde_json::json!({ "name": name, "exit_code": exit_code, "more": {} }).to_string()
    }

    fn evidence_within(lines: &[String], limit: u64) -> CallEvidence {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("log.jsonl");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&log, text).unwrap();
        calls_within(&log, limit, Counted::Any, wanted)
    }

    fn wanted(call: &Call) -> Option<u8> {
        (call.name == "wanted").then_some(call.exit_code)
    }

    fn evidence(lines: &[String]) -> CallEvidence {
        evidence_within(lines, MAX_LOG)
    }

    #[test]
    fn a_successful_call_succeeded() {
        let lines = [call("other", 1), call("wanted", 0)];

        assert_eq!(evidence(&lines), CallEvidence::Succeeded);
    }

    #[test]
    fn no_log_is_missing() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            calls(&dir.path().join("log.jsonl"), Counted::Any, wanted),
            CallEvidence::Missing
        );
    }

    #[test]
    fn a_log_without_the_call_is_missing() {
        let lines = [call("other", 0), String::new()];

        assert_eq!(evidence(&lines), CallEvidence::Missing);
    }

    #[test]
    fn a_call_that_exited_otherwise_failed() {
        assert_eq!(evidence(&[call("wanted", 2)]), CallEvidence::Failed);
    }

    #[test]
    fn a_later_success_settles_it() {
        let lines = [call("wanted", 1), "not json".to_owned(), call("wanted", 0)];

        assert_eq!(evidence(&lines), CallEvidence::Succeeded);
    }

    fn first_evidence(lines: &[String]) -> CallEvidence {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("log.jsonl");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&log, text).unwrap();
        calls(&log, Counted::First, wanted)
    }

    #[test]
    fn the_first_call_decides_when_it_is_the_one_counted() {
        let failed_first = [call("other", 0), call("wanted", 1), call("wanted", 0)];
        let succeeded_first = [call("other", 1), call("wanted", 0), call("wanted", 1)];

        assert_eq!(first_evidence(&failed_first), CallEvidence::Failed);
        assert_eq!(first_evidence(&succeeded_first), CallEvidence::Succeeded);
        assert_eq!(first_evidence(&[call("other", 0)]), CallEvidence::Missing);
    }

    #[test]
    fn an_unreadable_line_before_the_first_call_could_be_it() {
        let long = format!(r#"{{"pad":"{}"}}"#, "x".repeat(jsonl::MAX_LINE));
        for unreadable in ["not json".to_owned(), long] {
            let lines = [unreadable, call("wanted", 0)];

            assert_eq!(first_evidence(&lines), CallEvidence::Unknown);
        }
    }

    #[test]
    fn an_unreadable_line_after_the_first_call_does_not_matter() {
        let lines = [call("wanted", 1), "not json".to_owned()];

        assert_eq!(first_evidence(&lines), CallEvidence::Failed);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_unknown_even_beside_a_failure() {
        for unreadable in [
            "not json".to_owned(),
            r#"{"name":"wanted"}"#.to_owned(),
            r#"{"name":"wanted","exit_code":"0"}"#.to_owned(),
            r#"{"name":"wanted","exit_code":256}"#.to_owned(),
        ] {
            let lines = [call("wanted", 1), unreadable.clone()];

            assert_eq!(evidence(&lines), CallEvidence::Unknown, "{unreadable}");
        }
    }

    #[test]
    fn a_line_longer_than_any_read_is_unknown() {
        let long = format!(r#"{{"name":"x","pad":"{}"}}"#, "x".repeat(jsonl::MAX_LINE));

        assert_eq!(evidence(&[long]), CallEvidence::Unknown);
    }

    #[test]
    fn many_short_lines_past_the_limit_are_unknown() {
        let mut lines = vec![call("other", 0); 100];
        lines.push(call("wanted", 0));
        let size: usize = lines.iter().map(|line| line.len() + 1).sum();
        let size = size as u64;

        assert_eq!(evidence_within(&lines, size), CallEvidence::Succeeded);
        assert_eq!(evidence_within(&lines, size - 1), CallEvidence::Unknown);
    }

    #[test]
    fn a_log_that_grows_past_the_limit_while_read_is_unknown() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("log.jsonl");
        let line = call("other", 0) + "\n";
        fs::write(&log, line.repeat(10)).unwrap();

        // Ends the read on a whole line, so only the bound can make it unknown.
        let limit = (line.len() * 5 - 1) as u64;
        assert_eq!(
            read(&log, limit, Counted::Any, wanted).unwrap(),
            CallEvidence::Unknown
        );
    }

    #[test]
    fn a_directory_in_its_place_is_unknown() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("log.jsonl");
        fs::create_dir(&log).unwrap();

        assert_eq!(calls(&log, Counted::Any, wanted), CallEvidence::Unknown);
    }

    #[test]
    fn the_verdict_follows_from_the_evidence() {
        assert_eq!(CallEvidence::Succeeded.verdict(), Verdict::Pass);
        assert_eq!(CallEvidence::Failed.verdict(), Verdict::Fail);
        assert_eq!(CallEvidence::Missing.verdict(), Verdict::Fail);
        assert_eq!(CallEvidence::Unknown.verdict(), Verdict::Inconclusive);
    }

    /// Links need developer mode or elevation on Windows.
    #[cfg(unix)]
    #[test]
    fn a_linked_log_is_unknown() {
        let dir = TempDir::new().unwrap();
        let forged = dir.path().join("forged.jsonl");
        fs::write(&forged, call("wanted", 0) + "\n").unwrap();
        let log = dir.path().join("log.jsonl");
        std::os::unix::fs::symlink(&forged, &log).unwrap();

        assert_eq!(calls(&log, Counted::Any, wanted), CallEvidence::Unknown);
    }
}
