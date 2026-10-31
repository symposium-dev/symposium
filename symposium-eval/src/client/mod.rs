//! Running one client session in a trial (RFD reference, "Running the client").
//!
//! An adapter only builds its client's command line and reads its transcript. How the
//! process is sandboxed, stopped at the deadline, and logged is the [`Backend`]'s, so
//! adding a client means adding one small [`ClientRunner`].
//!
//! The transcript is the client's standard output, written by the runner to the trial's
//! `session/` from outside the sandbox. A client that exits 0 without a completion event,
//! or reports an error, has still failed. A client that never started, one the runner
//! lost hold of, and a transcript that cannot be read are not the client's failures, so
//! none is [`Ended::Failed`]. The exit status counts only once the transcript was read:
//! a client whose event format changed would otherwise read as a failed session.

mod claude;
mod error;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

pub(crate) use error::{ClientError, TranscriptError};

use crate::grade::ProcessError;
use crate::grade::process::{Backend, Invocation};
use crate::run::Trial;

/// One client, as the runner drives it.
pub(crate) trait ClientRunner {
    /// The command that starts a new session in `trial` and runs `prompt`, with `env` as
    /// its whole environment.
    fn invocation(&self, trial: &Trial, prompt: &str, env: Vec<(OsString, OsString)>)
    -> Invocation;

    /// How the session ended, as its transcript tells it.
    ///
    /// # Errors
    ///
    /// When the transcript cannot be read, or an event the adapter needs is malformed.
    fn completion(&self, transcript: &Path) -> Result<Completion, TranscriptError>;
}

/// What a transcript the adapter fully read says about how its session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Completion {
    Finished,
    /// The client reported an error, in its own words.
    Reported(String),
    /// No completion event.
    Missing,
}

/// One session's run: where its transcript is, and how it ended.
#[derive(Debug)]
pub(crate) struct Execution {
    /// Missing when the client never started.
    pub(crate) transcript: PathBuf,
    pub(crate) ended: Ended,
}

#[derive(Debug)]
pub(crate) enum Ended {
    /// The client finished without an error. This says nothing about the code.
    Completed,
    /// The deadline passed, and the runner killed the process tree.
    TimedOut {
        /// A problem collecting the output afterwards; the timeout still stands.
        after_deadline: Option<ProcessError>,
    },
    Failed(ClientError),
    /// The client never started: a setup failure, not a trial.
    NotLaunched(ProcessError),
    /// The runner could not supervise the client once it started.
    Unsupervised(ProcessError),
    /// The client exited, but its transcript cannot say how the session ended, whatever
    /// the exit status.
    Unreadable(TranscriptError),
}

/// Runs `prompt` as a new session of `runner`'s client in `trial`, through `backend`,
/// until it ends or `deadline` passes. Never an error: whatever goes wrong is how the
/// session ended.
pub(crate) fn execute(
    runner: &impl ClientRunner,
    backend: &impl Backend,
    trial: &Trial,
    env: Vec<(OsString, OsString)>,
    prompt: &str,
    deadline: Duration,
) -> Execution {
    let invocation = runner.invocation(trial, prompt, env);
    let logs = trial.session();
    let transcript = invocation.log(&logs, "stdout");
    let ended = match backend.run(&invocation, deadline, &logs) {
        // Every other error comes after the client started.
        Err(error @ ProcessError::Spawn { .. }) => Ended::NotLaunched(error),
        Err(error) => Ended::Unsupervised(error),
        Ok(finished) => match finished.status {
            None => Ended::TimedOut {
                after_deadline: finished.after_deadline,
            },
            Some(status) => match runner.completion(&transcript) {
                Err(error) => Ended::Unreadable(error),
                Ok(completion) => ending(status, completion, || {
                    finished.stderr.tail().trim().to_owned()
                }),
            },
        },
    };
    Execution { transcript, ended }
}

/// How a session that exited with `status` ended, by its transcript's `completion`. The
/// client's own error message, when it gave one, says more than its exit status.
fn ending(status: ExitStatus, completion: Completion, stderr: impl FnOnce() -> String) -> Ended {
    match (completion, status.success()) {
        (Completion::Reported(message), _) => Ended::Failed(ClientError::Reported(message)),
        (Completion::Finished, true) => Ended::Completed,
        (Completion::Missing, true) => Ended::Failed(ClientError::NoCompletion),
        (_, false) => Ended::Failed(ClientError::Exited {
            status,
            stderr: stderr(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::grade::process::Finished;
    use crate::run::test_support::{Host, trial};
    use crate::run::{Configuration, Reach};

    /// A client whose transcript is one line: `finished`, an error message, `garbled` for
    /// one it cannot read, or anything else for a missing completion.
    struct OneLine;

    impl ClientRunner for OneLine {
        fn invocation(
            &self,
            trial: &Trial,
            prompt: &str,
            env: Vec<(OsString, OsString)>,
        ) -> Invocation {
            Invocation {
                program: "one-line".into(),
                args: vec![prompt.into()],
                dir: trial.project(),
                env,
                log_name: "client".to_owned(),
            }
        }

        fn completion(&self, transcript: &Path) -> Result<Completion, TranscriptError> {
            let text = fs::read_to_string(transcript).map_err(|source| TranscriptError::Read {
                path: transcript.to_owned(),
                source,
            })?;
            Ok(match text.trim() {
                "finished" => Completion::Finished,
                line if line.starts_with("error: ") => {
                    Completion::Reported(line.trim_start_matches("error: ").to_owned())
                }
                "garbled" => {
                    return Err(TranscriptError::Malformed {
                        line: 1,
                        reason: "not an event".to_owned(),
                    });
                }
                _ => Completion::Missing,
            })
        }
    }

    /// Writes `transcript` where a real backend writes standard output, then ends as
    /// `status` says; `None` for the deadline, after which the output may stay open.
    struct Replay {
        transcript: &'static str,
        status: Option<i32>,
        output_stayed_open: bool,
    }

    impl Backend for Replay {
        fn run(
            &self,
            invocation: &Invocation,
            _deadline: Duration,
            logs: &Path,
        ) -> Result<Finished, ProcessError> {
            fs::write(invocation.log(logs, "stdout"), self.transcript).unwrap();
            let mut finished = Finished::never_started();
            finished.status = self.status.map(exit_status);
            finished.after_deadline = self
                .output_stayed_open
                .then_some(ProcessError::OutputNotClosed);
            Ok(finished)
        }
    }

    /// Exits with its status without writing a transcript.
    struct Silent(i32);

    impl Backend for Silent {
        fn run(&self, _: &Invocation, _: Duration, _: &Path) -> Result<Finished, ProcessError> {
            let mut finished = Finished::never_started();
            finished.status = Some(exit_status(self.0));
            Ok(finished)
        }
    }

    /// Fails with the error its function makes.
    struct Failing(fn() -> ProcessError);

    impl Backend for Failing {
        fn run(&self, _: &Invocation, _: Duration, _: &Path) -> Result<Finished, ProcessError> {
            Err(self.0())
        }
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        std::os::windows::process::ExitStatusExt::from_raw(code as u32)
    }

    /// The host comes back too: dropping it removes the trial.
    fn run(backend: &impl Backend) -> (Host, Trial, Execution) {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let execution = execute(
            &OneLine,
            backend,
            &trial,
            Vec::new(),
            "do the task",
            Duration::from_secs(60),
        );
        (host, trial, execution)
    }

    fn replay(transcript: &'static str, status: Option<i32>) -> Ended {
        run(&Replay {
            transcript,
            status,
            output_stayed_open: false,
        })
        .2
        .ended
    }

    #[test]
    fn a_finished_session_is_completed_and_its_transcript_kept() {
        let (_host, trial, execution) = run(&Replay {
            transcript: "finished\n",
            status: Some(0),
            output_stayed_open: false,
        });

        assert!(matches!(execution.ended, Ended::Completed), "{execution:?}");
        assert!(execution.transcript.starts_with(trial.session()));
        assert_eq!(
            fs::read_to_string(&execution.transcript).unwrap(),
            "finished\n"
        );
    }

    #[test]
    fn exiting_0_without_a_completion_event_is_a_client_error() {
        let ended = replay("thinking\n", Some(0));

        assert!(
            matches!(ended, Ended::Failed(ClientError::NoCompletion)),
            "{ended:?}"
        );
    }

    #[test]
    fn a_reported_error_is_a_client_error_whatever_the_exit() {
        for status in [0, 1] {
            let ended = replay("error: rate limited\n", Some(status));

            assert!(
                matches!(&ended, Ended::Failed(ClientError::Reported(message)) if message == "rate limited"),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn a_failed_exit_is_a_client_error_even_after_a_completion_event() {
        let ended = replay("finished\n", Some(1));

        assert!(
            matches!(ended, Ended::Failed(ClientError::Exited { .. })),
            "{ended:?}"
        );
    }

    #[test]
    fn the_deadline_is_a_timeout_and_the_transcript_is_still_kept() {
        let (_host, _, execution) = run(&Replay {
            transcript: "thinking\n",
            status: None,
            output_stayed_open: false,
        });

        assert!(
            matches!(
                execution.ended,
                Ended::TimedOut {
                    after_deadline: None
                }
            ),
            "{execution:?}"
        );
        assert!(execution.transcript.is_file());
    }

    #[test]
    fn a_collection_problem_after_the_deadline_is_kept_beside_the_timeout() {
        let (_host, _, execution) = run(&Replay {
            transcript: "thinking\n",
            status: None,
            output_stayed_open: true,
        });

        assert!(
            matches!(
                execution.ended,
                Ended::TimedOut {
                    after_deadline: Some(ProcessError::OutputNotClosed)
                }
            ),
            "{execution:?}"
        );
    }

    #[test]
    fn a_client_that_never_started_is_not_launched() {
        let (_host, _, execution) = run(&Failing(|| ProcessError::Spawn {
            program: "one-line".into(),
            source: std::io::ErrorKind::NotFound.into(),
        }));

        assert!(
            matches!(
                execution.ended,
                Ended::NotLaunched(ProcessError::Spawn { .. })
            ),
            "{execution:?}"
        );
    }

    #[test]
    fn losing_hold_of_a_started_client_is_the_runners_problem() {
        let (_host, _, execution) = run(&Failing(|| ProcessError::OutputNotClosed));

        assert!(
            matches!(
                execution.ended,
                Ended::Unsupervised(ProcessError::OutputNotClosed)
            ),
            "{execution:?}"
        );
    }

    #[test]
    fn a_malformed_transcript_is_unreadable_whatever_the_exit() {
        for status in [0, 1] {
            let ended = replay("garbled\n", Some(status));

            assert!(
                matches!(
                    ended,
                    Ended::Unreadable(TranscriptError::Malformed { line: 1, .. })
                ),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn a_transcript_the_runner_cannot_read_is_unreadable_whatever_the_exit() {
        for status in [0, 1] {
            let (_host, _, execution) = run(&Silent(status));

            assert!(
                matches!(
                    execution.ended,
                    Ended::Unreadable(TranscriptError::Read { .. })
                ),
                "{execution:?}"
            );
        }
    }

    #[test]
    fn the_trial_cannot_reach_its_session() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let reach = Reach::for_trial(&trial, &host.machine()).unwrap();

        let session = trial.session();
        assert!(session.is_dir());
        assert!(
            reach
                .allow_read
                .iter()
                .chain(&reach.allow_write)
                .all(|allowed| !session.starts_with(allowed) && !allowed.starts_with(&session)),
            "{reach:?}"
        );
        assert!(
            reach
                .deny_read
                .iter()
                .any(|denied| session.starts_with(denied))
        );
    }
}
