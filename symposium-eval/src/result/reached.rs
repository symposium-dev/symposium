//! What a result records of how far a trial got, and of its outcome.

use serde::Serialize;

use super::session::SessionRecord;
use super::{Reached, ResultError, SetupStep, sanitized};
use crate::client::{Ended, Execution};
use crate::expect::config::ConfigChange;
use crate::expect::decide::Expectations;
use crate::grade::Grading;
use crate::outcome::{self, Failure, Outcome, Session};
use crate::run::Configuration;
use crate::task::Task;

/// Where the trial stopped, and why when it stopped early.
#[derive(Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(super) enum ReachedRecord {
    Setup {
        error: String,
    },
    Prelaunch {
        error: String,
    },
    /// The client could not be launched; `session` says why.
    Launch,
    Graded,
}

#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub(super) enum OutcomeRecord {
    SetupFailure,
    IsolationExclusion,
    GradingExclusion,
    Succeeded,
    Failed { failure: FailureRecord },
    Inconclusive,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureRecord {
    Session,
    Submission,
    Code,
    Expectation,
}

impl<'a> Reached<'a> {
    pub(super) fn outcome(self, task: &Task) -> Outcome {
        match self {
            Self::Setup { .. } | Self::Session { .. } => {
                outcome::decide(task.kind(), Session::NotStarted, &[])
            }
            Self::Graded {
                execution,
                grading,
                expectations,
                ..
            } => {
                let verdicts = expectations.map(Expectations::verdicts).unwrap_or_default();
                let session = Session::Graded {
                    ended: &execution.ended,
                    grading,
                };
                outcome::decide(task.kind(), session, &verdicts)
            }
        }
    }

    pub(super) fn session(self, task: &Task) -> Result<Option<SessionRecord>, ResultError> {
        match self {
            Self::Session { execution, .. } | Self::Graded { execution, .. } => {
                SessionRecord::new(task, execution).map(Some)
            }
            Self::Setup { .. } => Ok(None),
        }
    }

    /// A session is left ungraded exactly when its client never launched.
    pub(super) fn check_session_phase(self) -> Result<(), ResultError> {
        let (launched, graded) = match self {
            Self::Session { execution, .. } => (launched(execution), false),
            Self::Graded { execution, .. } => (launched(execution), true),
            Self::Setup { .. } => return Ok(()),
        };
        if launched == graded {
            Ok(())
        } else {
            Err(ResultError::SessionPhase { launched })
        }
    }

    /// A graded trial has expectations exactly when it is a Symposium trial. One that
    /// stopped before launch has none, whatever its configuration.
    pub(super) fn check_expectations(
        self,
        configuration: Configuration,
    ) -> Result<(), ResultError> {
        let Self::Graded { expectations, .. } = self else {
            return Ok(());
        };
        let expected = configuration == Configuration::Symposium;
        match expectations.is_some() == expected {
            true => Ok(()),
            false => Err(ResultError::Expectations {
                configuration,
                expectations: expectations.is_some(),
            }),
        }
    }

    pub(super) fn grading(self) -> Option<&'a Grading> {
        match self {
            Self::Graded { grading, .. } => Some(grading),
            Self::Setup { .. } | Self::Session { .. } => None,
        }
    }

    pub(super) fn expectations(self) -> Option<&'a Expectations> {
        match self {
            Self::Graded { expectations, .. } => expectations,
            Self::Setup { .. } | Self::Session { .. } => None,
        }
    }

    pub(super) fn changes(self) -> &'a [ConfigChange] {
        match self {
            Self::Session { changes, .. } | Self::Graded { changes, .. } => changes,
            Self::Setup { .. } => &[],
        }
    }

    pub(super) fn reported_model(self) -> Option<String> {
        match self {
            Self::Session { execution, .. } | Self::Graded { execution, .. } => {
                execution.summary.model.clone()
            }
            Self::Setup { .. } => None,
        }
    }
}

fn launched(execution: &Execution) -> bool {
    !matches!(execution.ended, Ended::NotLaunched(_))
}

impl From<Reached<'_>> for ReachedRecord {
    fn from(reached: Reached<'_>) -> Self {
        match reached {
            Reached::Setup {
                step: SetupStep::Setup,
                error,
            } => Self::Setup {
                error: sanitized(error),
            },
            Reached::Setup {
                step: SetupStep::Prelaunch,
                error,
            } => Self::Prelaunch {
                error: sanitized(error),
            },
            Reached::Session { .. } => Self::Launch,
            Reached::Graded { .. } => Self::Graded,
        }
    }
}

impl From<Outcome> for OutcomeRecord {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::SetupFailure => Self::SetupFailure,
            Outcome::IsolationExclusion => Self::IsolationExclusion,
            Outcome::GradingExclusion => Self::GradingExclusion,
            Outcome::Succeeded => Self::Succeeded,
            Outcome::Inconclusive => Self::Inconclusive,
            Outcome::Failed(failure) => Self::Failed {
                failure: FailureRecord::from(failure),
            },
        }
    }
}

impl From<Failure> for FailureRecord {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Session => Self::Session,
            Failure::Submission => Self::Submission,
            Failure::Code => Self::Code,
            Failure::Expectation => Self::Expectation,
        }
    }
}
