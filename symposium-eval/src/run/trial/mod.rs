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
mod sandbox;
#[cfg(test)]
mod tests;

use std::io;
use std::path::PathBuf;

use thiserror::Error;

pub(crate) use sandbox::Sandbox;

use super::{ApiKey, Batch, Inputs, Machine, RunError};
use crate::client::{ClientRunner, Execution};
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
    /// Returns where the result is.
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
    ) -> Result<PathBuf, TrialError> {
        let new = self
            .batch
            .allocate_trial(number, arm.configuration(), self.inputs)
            .map_err(TrialError::Allocate)?;
        let path = new.root().join(RESULT_FILE);
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
        result.write(&path).map_err(|source| TrialError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }
}

impl Ran {
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
