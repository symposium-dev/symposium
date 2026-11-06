//! Running one trial end to end (RFD reference, "Running a trial").
//!
//! The trial's root is allocated first, so whatever happens next has a place for its
//! result. Then each phase runs in turn: preparing the trial, setting up Symposium, the
//! checks before launch, the session, gathering what Symposium left, grading, and
//! deciding expectations. A phase that fails stops the trial, and its result says where.
//! Only a root that cannot be allocated, parts that cannot make one result, or a result
//! that cannot be written are the runner's own errors.
//!
//! Symposium's evidence is gathered before grading, which needs it to tell the skill
//! copies sync installed from the agent's own edits.

mod phases;
#[cfg(all(test, target_os = "linux"))]
mod real;
mod retain;
mod sandbox;
#[cfg(test)]
mod tests;

use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub(crate) use sandbox::Sandbox;

use super::{ApiKey, Batch, Configuration, Inputs, Machine, RunError};
use crate::client::{ClientRunner, Ended, Execution};
use crate::expect::config::ConfigChange;
use crate::expect::decide::Expectations;
use crate::grade::Grading;
use crate::result::{Arm, Planned, Provenance, Reached, ResultError, SetupStep, TrialResult};

/// Where a trial's result is written, in its root.
pub(crate) const RESULT_FILE: &str = "result.json";

/// What every trial of a batch shares.
#[derive(Debug)]
pub(crate) struct Driver<'a, S> {
    pub(crate) batch: &'a Batch,
    pub(crate) inputs: &'a Inputs,
    pub(crate) machine: &'a Machine,
    pub(crate) sandbox: &'a S,
    /// The only host a trial's network reaches.
    pub(crate) api_host: &'a str,
    pub(crate) api_key: &'a ApiKey,
    pub(crate) provenance: &'a Provenance,
}

/// A trial whose result is written, with what the runner keeps of it.
#[derive(Debug)]
pub(crate) struct Done {
    /// Canonical runner-owned directory above every batch and trial.
    pub(crate) anchor: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) configuration: Configuration,
    /// What the client left, when it was launched.
    pub(crate) session: Option<SessionOutput>,
    /// Every process tree that could write the trial has been stopped and reaped.
    pub(crate) quiescent: bool,
}

/// A launched client's output.
#[derive(Debug)]
pub(crate) struct SessionOutput {
    pub(crate) transcript: PathBuf,
    pub(crate) stderr: PathBuf,
    /// The agent's last message, from a session that finished.
    pub(crate) response: Option<String>,
}

impl Done {
    pub(crate) fn result(&self) -> PathBuf {
        self.root.join(RESULT_FILE)
    }
}

/// Why a trial has no result.
#[derive(Debug, Error)]
pub(crate) enum TrialError {
    #[error("cannot allocate the trial")]
    Allocate(#[source] RunError),
    #[error("the trial's parts cannot make one result")]
    Result(#[source] ResultError),
    #[error("cannot write `{}`", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// How far a trial got, owning what each phase left.
#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "one per trial, never collected")]
enum Ran {
    Setup(Stopped),
    Session {
        execution: Execution,
        changes: Vec<ConfigChange>,
    },
    Graded {
        execution: Execution,
        grading: Grading,
        changes: Vec<ConfigChange>,
        expectations: Option<Expectations>,
    },
}

/// A trial stopped before its client launched.
#[derive(Debug)]
struct Stopped {
    step: SetupStep,
    error: RunError,
}

impl<S: Sandbox> Driver<'_, S> {
    /// Runs trial `number` of `arm` with `runner`'s client, and writes its result.
    ///
    /// # Errors
    ///
    /// When the trial cannot be allocated, its parts cannot make one result, or the
    /// result cannot be written. Anything else that goes wrong is in the result.
    pub(crate) fn run(
        &self,
        number: usize,
        arm: Arm,
        runner: &impl ClientRunner,
    ) -> Result<Done, TrialError> {
        let configuration = arm.configuration();
        let new = self
            .batch
            .allocate_trial(number, configuration, self.inputs)
            .map_err(TrialError::Allocate)?;
        let root = new.root().to_owned();
        let path = root.join(RESULT_FILE);
        let planned = Planned {
            task: self.inputs.task(),
            content_hash: self.inputs.digest(),
            client: runner.client(),
            model: runner.model(),
            arm,
            trial: number,
        };
        let ran = self.ran(new, runner);
        let result = TrialResult::new(planned, self.provenance.clone(), ran.reached())
            .map_err(TrialError::Result)?;
        result
            .write(&path)
            .map_err(|source| TrialError::Write { path, source })?;
        let quiescent = ran.quiescent();
        Ok(Done {
            anchor: self.batch.eval_root().to_owned(),
            root,
            configuration,
            session: ran.into_session(),
            quiescent,
        })
    }
}

impl Ran {
    /// Whether every process tree that could still alter the trial was stopped.
    fn quiescent(&self) -> bool {
        match self {
            Self::Setup(Stopped {
                error:
                    RunError::Process { .. }
                    | RunError::ToolTimeout {
                        after_deadline: Some(_),
                        ..
                    },
                ..
            }) => false,
            Self::Setup(_) => true,
            Self::Session { execution, .. } => execution_is_quiescent(execution),
            Self::Graded {
                execution, grading, ..
            } => execution_is_quiescent(execution) && grading_is_quiescent(grading),
        }
    }

    /// What the client left, when it was launched.
    fn into_session(self) -> Option<SessionOutput> {
        let execution = match self {
            Self::Setup(_) => return None,
            Self::Session { execution, .. } | Self::Graded { execution, .. } => execution,
        };
        if matches!(execution.ended, Ended::NotLaunched(_)) {
            return None;
        }
        Some(SessionOutput {
            transcript: execution.transcript,
            stderr: execution.stderr,
            response: execution.summary.response,
        })
    }

    fn reached(&self) -> Reached<'_> {
        match self {
            Self::Setup(stopped) => Reached::Setup {
                step: stopped.step,
                error: &stopped.error,
            },
            Self::Session { execution, changes } => Reached::Session { execution, changes },
            Self::Graded {
                execution,
                grading,
                changes,
                expectations,
            } => Reached::Graded {
                execution,
                grading,
                changes,
                expectations: expectations.as_ref(),
            },
        }
    }
}

fn execution_is_quiescent(execution: &Execution) -> bool {
    !matches!(
        execution.ended,
        Ended::Unsupervised(_)
            | Ended::TimedOut {
                after_deadline: Some(_)
            }
    )
}

fn grading_is_quiescent(grading: &Grading) -> bool {
    grading.containment_is_certain()
}
