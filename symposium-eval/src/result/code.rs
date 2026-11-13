//! What a result records of grading.

use std::collections::BTreeMap;

use serde::Serialize;

use super::capped;
use crate::grade::{self, Grading, Outcome as Check, Report, StepKind};

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum CodeRecord {
    Graded(ChecksRecord),
    /// The submission could not be graded, which fails the trial.
    InvalidSubmission {
        error: String,
    },
    /// Grading itself failed.
    InfrastructureError {
        error: String,
    },
}

#[derive(Debug, Serialize)]
pub(super) struct ChecksRecord {
    /// `skipped` only if grading ran no build.
    build: CheckRecord,
    cargo_test_lib: Option<CheckRecord>,
    hidden: BTreeMap<String, CheckRecord>,
    /// `null` when the task does not check formatting.
    cargo_fmt_check: Option<CheckRecord>,
    edits_outside_editable: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckRecord {
    Pass,
    Fail,
    TimedOut,
    Unconfirmed,
    Skipped,
}

impl From<&Grading> for CodeRecord {
    fn from(grading: &Grading) -> Self {
        match grading {
            Grading::Graded(report) => Self::Graded(ChecksRecord::from(report)),
            Grading::InvalidSubmission(error) => Self::InvalidSubmission {
                error: capped(grade::with_causes(error)),
            },
            Grading::Infrastructure(error) => Self::InfrastructureError {
                error: capped(grade::with_causes(error)),
            },
        }
    }
}

impl From<&Report> for ChecksRecord {
    fn from(report: &Report) -> Self {
        let mut checks = Self {
            build: CheckRecord::Skipped,
            cargo_test_lib: None,
            hidden: BTreeMap::new(),
            cargo_fmt_check: None,
            edits_outside_editable: report
                .outside_editable
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        for step in &report.steps {
            let check = CheckRecord::from(&step.outcome);
            match &step.kind {
                StepKind::Build => checks.build = check,
                StepKind::TestLib => checks.cargo_test_lib = Some(check),
                StepKind::Hidden(name) => {
                    checks.hidden.insert(name.to_string(), check);
                }
                StepKind::Fmt => checks.cargo_fmt_check = Some(check),
            }
        }
        checks
    }
}

impl From<&Check> for CheckRecord {
    fn from(check: &Check) -> Self {
        match check {
            Check::Passed => Self::Pass,
            Check::Failed { .. } => Self::Fail,
            Check::TimedOut { .. } => Self::TimedOut,
            Check::Unconfirmed { .. } => Self::Unconfirmed,
            Check::Skipped => Self::Skipped,
        }
    }
}
