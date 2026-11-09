//! A trial's `result.json` (RFD reference, "Retained results").
//!
//! As cargo, rustc and nextest do with their machine-readable output, the schema is its
//! own set of types, built from the runner's by small conversions, so renaming a type
//! inside the runner never changes it. Each file holds one part's types with their
//! conversions.
//!
//! What a trial always has is always present: the planned task, client, model,
//! configuration and trial number, and the provenance known before launch. A phase's
//! output is `null` exactly when the trial stopped before it, and `reached` says where
//! it stopped and why. A value a phase reached but could not know is said so with its
//! own variant, never left `null`.

mod code;
mod concern;
mod expectations;
mod provenance;
mod reached;
mod session;
#[cfg(test)]
mod tests;
mod write;

use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

use self::code::CodeRecord;
use self::expectations::ExpectationsRecord;
use self::provenance::{
    ClientRecord, DeadlinesRecord, IsolationRecord, ModelRecord, SymposiumRecord, TaskRecord,
};
use self::reached::{OutcomeRecord, ReachedRecord};
use self::session::SessionRecord;
use crate::client::Execution;
use crate::digest::Digest;
use crate::expect::config::ConfigChange;
use crate::expect::decide::Expectations;
use crate::grade::Grading;
use crate::run::Configuration;
use crate::run::client::Client;
use crate::task::Task;

/// Bumped only when a field is removed or changes meaning. A new field is additive and
/// keeps the version, so a reader must ignore fields it does not know.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// The longest error message kept, in bytes, its cut marker included; the logs keep
/// all of it.
const MAX_ERROR: usize = 4096;

/// What ends an error cut to [`MAX_ERROR`].
const CUT: &str = " [cut]";

/// What a trial was planned to be.
#[derive(Debug, Clone)]
pub(crate) struct Planned<'a> {
    pub(crate) task: &'a Task,
    pub(crate) content_hash: Digest,
    pub(crate) client: Client,
    pub(crate) model: &'a str,
    pub(crate) arm: Arm,
    /// The round this arm belongs to, shared by paired baseline and Symposium trials.
    pub(crate) trial: usize,
}

/// Which configuration a trial ran, with the `cargo-agents` a Symposium trial ran, which
/// is known before any trial launches. A baseline trial cannot carry one.
#[derive(Debug, Clone)]
pub(crate) enum Arm {
    Baseline,
    Symposium(SymposiumBuild),
}

impl Arm {
    pub(crate) fn configuration(&self) -> Configuration {
        match self {
            Self::Baseline => Configuration::Baseline,
            Self::Symposium(_) => Configuration::Symposium,
        }
    }
}

/// What ran it, as known before launch.
#[derive(Debug, Clone)]
pub(crate) struct Provenance {
    /// `None` when preflight could not find it.
    pub(crate) client_version: Option<String>,
    pub(crate) os: String,
    pub(crate) toolchain: Option<String>,
    pub(crate) isolation: IsolationMode,
    pub(crate) srt_version: Option<String>,
    pub(crate) deadlines: Deadlines,
}

/// The `cargo-agents` a Symposium trial ran.
#[derive(Debug, Clone)]
pub(crate) struct SymposiumBuild {
    pub(crate) version: String,
    pub(crate) sha256: Digest,
    /// The checkout's revision, when the binary was built from it.
    pub(crate) revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IsolationMode {
    Enforced,
    #[expect(dead_code, reason = "--exploratory needs its own design")]
    Exploratory,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Deadlines {
    pub(crate) setup: Duration,
    pub(crate) agent: Duration,
    pub(crate) grading: Duration,
}

/// The setup step a trial stopped at, before its client launched.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SetupStep {
    /// Creating the trial or setting up Symposium.
    Setup,
    /// The checks before launch.
    Prelaunch,
}

/// How far a trial got, with what each phase it reached left.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Reached<'a> {
    Setup {
        step: SetupStep,
        error: &'a dyn std::error::Error,
    },
    /// The client could not be launched, so nothing was graded: a setup failure, with
    /// the attempt kept.
    Session {
        execution: &'a Execution,
        changes: &'a [ConfigChange],
    },
    Graded {
        execution: &'a Execution,
        grading: &'a Grading,
        changes: &'a [ConfigChange],
        /// A Symposium trial's, and only a Symposium trial's.
        expectations: Option<&'a Expectations>,
    },
}

/// Parts that cannot belong to one trial, which a result refuses rather than records.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ResultError {
    #[error("a {configuration} trial was graded {}", if *.expectations { "with expectations" } else { "without them" })]
    Expectations {
        configuration: Configuration,
        expectations: bool,
    },
    #[error("the session looked for {named} skills but showed {observed} loads")]
    Loads { named: usize, observed: usize },
    #[error("a client that {} was {}", if *.launched { "launched" } else { "never launched" }, if *.launched { "left ungraded" } else { "graded" })]
    SessionPhase { launched: bool },
}

#[derive(Debug, Serialize)]
pub(crate) struct TrialResult {
    schema_version: u32,
    task: TaskRecord,
    client: ClientRecord,
    model: ModelRecord,
    mode: String,
    trial: usize,
    symposium: Option<SymposiumRecord>,
    os: String,
    toolchain: Option<String>,
    deadlines_secs: DeadlinesRecord,
    isolation: IsolationRecord,
    reached: ReachedRecord,
    outcome: OutcomeRecord,
    session: Option<SessionRecord>,
    code: Option<CodeRecord>,
    expectations: Option<ExpectationsRecord>,
}

impl TrialResult {
    /// The result of a trial planned as `planned`, run with `provenance`, that got as far
    /// as `reached`. Its outcome is decided here, so it always agrees with the phases.
    ///
    /// # Errors
    ///
    /// When `reached` cannot belong to the trial `planned` describes.
    pub(crate) fn new(
        planned: Planned<'_>,
        provenance: Provenance,
        reached: Reached<'_>,
    ) -> Result<Self, ResultError> {
        let task = planned.task;
        let configuration = planned.arm.configuration();
        reached.check_expectations(configuration)?;
        reached.check_session_phase()?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            task: TaskRecord::new(task, planned.content_hash),
            client: ClientRecord::new(planned.client, provenance.client_version),
            model: ModelRecord::new(planned.model, reached.reported_model()),
            mode: configuration.to_string(),
            trial: planned.trial,
            symposium: match planned.arm {
                Arm::Symposium(build) => Some(SymposiumRecord::from(build)),
                Arm::Baseline => None,
            },
            os: provenance.os,
            toolchain: provenance.toolchain,
            deadlines_secs: DeadlinesRecord::from(provenance.deadlines),
            isolation: IsolationRecord::new(
                provenance.isolation,
                provenance.srt_version,
                reached.changes(),
            ),
            reached: ReachedRecord::from(reached),
            outcome: OutcomeRecord::from(reached.outcome(task)),
            session: reached.session(task)?,
            code: reached.grading().map(CodeRecord::from),
            expectations: reached.expectations().map(ExpectationsRecord::from),
        })
    }
}

/// An error with its causes, cut to [`MAX_ERROR`] bytes.
fn sanitized(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        cause = inner.source();
    }
    capped(text)
}

/// `text`, cut on a character boundary to at most [`MAX_ERROR`] bytes, marker included.
fn capped(mut text: String) -> String {
    if text.len() > MAX_ERROR {
        let end = text.floor_char_boundary(MAX_ERROR - CUT.len());
        text.truncate(end);
        text.push_str(CUT);
    }
    text
}
