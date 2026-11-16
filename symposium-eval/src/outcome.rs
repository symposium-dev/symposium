//! Where a trial ends in the count (RFD reference, "Outcomes and counting").
//!
//! [`decide`] reads nothing: it takes how the session ended, its grading and its
//! expectations' verdicts, so every rule is tested on its own facts. Its order is the
//! precedence: a trial that never launched is a setup failure; one that broke isolation
//! is excluded whatever it produced; a session that did not complete fails, whatever
//! grading finds; only then do the code and expectations decide, and grading's own
//! failure excludes the trial only when grading was needed to decide it.
//!
//! A session the runner lost hold of, or whose ending its transcript cannot show, did
//! not complete either. Excluding it instead would let a configuration that breaks the
//! runner's supervision, such as with a process left holding the client's output, score
//! better by shrinking its count.

use crate::client::Ended;
use crate::expect::Verdict;
use crate::grade::{Grading, Outcome as Check, Report};
use crate::task::TaskKind;

/// Where a planned trial ends. Cancellation is decided by the batch, before a trial
/// runs, so it is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The client never launched.
    SetupFailure,
    /// The client launched, but the trial violated isolation.
    IsolationExclusion,
    /// Grading was needed to decide, and grading itself failed.
    GradingExclusion,
    Succeeded,
    Failed(Failure),
    /// A regression trial in which an expectation could not be decided and nothing
    /// failed.
    Inconclusive,
}

/// Why a trial failed, by the first rule it broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The session did not complete: it timed out, the client reported an error, or
    /// the runner lost hold of it or could not read how it ended.
    Session,
    /// The submission could not be graded.
    Submission,
    /// A code check did not pass.
    Code,
    /// An expectation failed.
    Expectation,
}

/// How far a trial got.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Session<'a> {
    /// Setup or the pre-launch check failed, so the client was never started.
    NotStarted,
    /// The session ended, and its submission was graded.
    Graded {
        ended: &'a Ended,
        grading: &'a Grading,
    },
}

/// What the code checks say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    Passed,
    Failed(Failure),
    /// Grading itself failed.
    Unknown,
}

/// Where a trial of a `kind` task ends, from how far it got and the verdicts of its
/// expectations, which a baseline trial has none of.
pub(crate) fn decide(kind: TaskKind, session: Session<'_>, expectations: &[Verdict]) -> Outcome {
    let Session::Graded { ended, grading } = session else {
        return Outcome::SetupFailure;
    };
    match ended {
        Ended::NotLaunched(_) => Outcome::SetupFailure,
        Ended::Mismatched(_) => Outcome::IsolationExclusion,
        Ended::TimedOut { .. }
        | Ended::Failed(_)
        | Ended::Unsupervised(_)
        | Ended::Unreadable(_) => Outcome::Failed(Failure::Session),
        Ended::Completed => completed(kind, code(grading), expectations),
    }
}

/// A completed session's outcome: a regression task's expectations decide with its
/// code, and a capability task's are only reported.
fn completed(kind: TaskKind, code: Code, expectations: &[Verdict]) -> Outcome {
    let any = |verdict| expectations.contains(&verdict);
    match (kind, code) {
        (_, Code::Failed(failure)) => Outcome::Failed(failure),
        (TaskKind::Regression, _) if any(Verdict::Fail) => Outcome::Failed(Failure::Expectation),
        (_, Code::Unknown) => Outcome::GradingExclusion,
        (TaskKind::Regression, Code::Passed) if any(Verdict::Inconclusive) => Outcome::Inconclusive,
        (_, Code::Passed) => Outcome::Succeeded,
    }
}

fn code(grading: &Grading) -> Code {
    match grading {
        Grading::Graded(report) if passed(report) => Code::Passed,
        Grading::Graded(_) => Code::Failed(Failure::Code),
        Grading::InvalidSubmission(_) => Code::Failed(Failure::Submission),
        Grading::Infrastructure { .. } => Code::Unknown,
    }
}

/// Every check ran and passed. Edits outside `editable` are reported, never graded.
fn passed(report: &Report) -> bool {
    report
        .steps
        .iter()
        .all(|step| step.outcome == Check::Passed)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::client::{ClientError, ProvenanceError, TranscriptError};
    use crate::grade::{GradeError, ImportError, ProcessError, Step, StepKind};
    use crate::tree::EntryKind;

    use TaskKind::{Capability, Regression};
    use Verdict::{Fail, Inconclusive, Pass};

    fn report(outcome: Check) -> Grading {
        Grading::Graded(Report {
            outside_editable: Vec::new(),
            steps: vec![
                Step {
                    kind: StepKind::Build,
                    outcome: Check::Passed,
                },
                Step {
                    kind: StepKind::TestLib,
                    outcome,
                },
            ],
        })
    }

    fn passing() -> Grading {
        report(Check::Passed)
    }

    fn failing() -> Grading {
        report(Check::TimedOut {
            after_deadline: None,
        })
    }

    fn invalid() -> Grading {
        Grading::InvalidSubmission(ImportError::SubmissionRoot(EntryKind::Symlink))
    }

    fn broken() -> Grading {
        Grading::Infrastructure {
            error: GradeError::Process {
                step: "cargo build".to_owned(),
                source: process_error(),
            },
            partial: None,
        }
    }

    fn process_error() -> ProcessError {
        ProcessError::Spawn {
            program: "client".into(),
            source: io::Error::other("not found"),
        }
    }

    fn graded<'a>(ended: &'a Ended, grading: &'a Grading) -> Session<'a> {
        Session::Graded { ended, grading }
    }

    #[test]
    fn a_trial_that_never_launched_is_a_setup_failure() {
        let not_launched = Ended::NotLaunched(process_error());

        assert_eq!(
            decide(Regression, Session::NotStarted, &[]),
            Outcome::SetupFailure
        );
        assert_eq!(
            decide(Regression, graded(&not_launched, &passing()), &[Pass]),
            Outcome::SetupFailure
        );
    }

    #[test]
    fn broken_isolation_excludes_the_trial_whatever_it_produced() {
        let mismatched = Ended::Mismatched(ProvenanceError::Unstated);

        for kind in [Regression, Capability] {
            for grading in [passing(), failing(), broken()] {
                assert_eq!(
                    decide(kind, graded(&mismatched, &grading), &[Fail]),
                    Outcome::IsolationExclusion
                );
            }
        }
    }

    #[test]
    fn a_session_that_did_not_complete_fails_even_when_grading_breaks() {
        // The runner losing hold of a session counts too: excluding it would let a
        // configuration that breaks supervision score better.
        let ended = [
            Ended::TimedOut {
                after_deadline: None,
            },
            Ended::Failed(ClientError::NoCompletion),
            Ended::Unsupervised(ProcessError::OutputNotClosed),
            Ended::Unreadable(TranscriptError::Read {
                path: "transcript.jsonl".into(),
                source: io::Error::other("gone"),
            }),
        ];

        for ended in &ended {
            for kind in [Regression, Capability] {
                for grading in [passing(), broken()] {
                    assert_eq!(
                        decide(kind, graded(ended, &grading), &[Pass]),
                        Outcome::Failed(Failure::Session)
                    );
                }
            }
        }
    }

    #[test]
    fn a_regression_trial_needs_its_code_and_every_expectation() {
        let completed = Ended::Completed;
        let regression = |grading: &Grading, expectations: &[Verdict]| {
            decide(Regression, graded(&completed, grading), expectations)
        };

        assert_eq!(regression(&passing(), &[Pass, Pass]), Outcome::Succeeded);
        assert_eq!(
            regression(&passing(), &[Pass, Fail]),
            Outcome::Failed(Failure::Expectation)
        );
        assert_eq!(
            regression(&passing(), &[Inconclusive, Pass]),
            Outcome::Inconclusive
        );
        assert_eq!(
            regression(&failing(), &[Inconclusive]),
            Outcome::Failed(Failure::Code)
        );
        assert_eq!(
            regression(&invalid(), &[Pass]),
            Outcome::Failed(Failure::Submission)
        );
    }

    #[test]
    fn broken_grading_excludes_only_a_trial_it_was_needed_to_decide() {
        let completed = Ended::Completed;
        let decided = |kind, expectations: &[Verdict]| {
            decide(kind, graded(&completed, &broken()), expectations)
        };

        // A failed expectation already fails the trial, whatever the code would show.
        assert_eq!(
            decided(Regression, &[Pass, Fail]),
            Outcome::Failed(Failure::Expectation)
        );
        // A code failure could still fail it, or settle an inconclusive one.
        assert_eq!(decided(Regression, &[Pass]), Outcome::GradingExclusion);
        assert_eq!(
            decided(Regression, &[Inconclusive]),
            Outcome::GradingExclusion
        );
        // The code alone decides a capability trial.
        assert_eq!(decided(Capability, &[Fail]), Outcome::GradingExclusion);
    }

    #[test]
    fn a_capability_trial_is_decided_by_its_code_alone() {
        let completed = Ended::Completed;
        let capability = |grading: &Grading, expectations: &[Verdict]| {
            decide(Capability, graded(&completed, grading), expectations)
        };

        assert_eq!(
            capability(&passing(), &[Fail, Inconclusive]),
            Outcome::Succeeded
        );
        assert_eq!(
            capability(&failing(), &[Pass]),
            Outcome::Failed(Failure::Code)
        );
        assert_eq!(
            capability(&invalid(), &[Pass]),
            Outcome::Failed(Failure::Submission)
        );
    }

    #[test]
    fn a_baseline_trial_has_only_its_code() {
        let completed = Ended::Completed;

        assert_eq!(
            decide(Regression, graded(&completed, &passing()), &[]),
            Outcome::Succeeded
        );
        assert_eq!(
            decide(Regression, graded(&completed, &broken()), &[]),
            Outcome::GradingExclusion
        );
    }

    #[test]
    fn every_check_must_pass() {
        let skipped = report(Check::Skipped);
        let unconfirmed = report(Check::Unconfirmed {
            stdout: String::new(),
            stderr: String::new(),
            truncated: false,
        });

        for grading in [skipped, unconfirmed, failing()] {
            assert_eq!(code(&grading), Code::Failed(Failure::Code));
        }
    }
}
