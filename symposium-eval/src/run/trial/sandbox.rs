//! Where a trial's commands run. Every run confines them with `srt`; only tests supply
//! another sandbox, so nothing outside a test can run a trial unconfined.

use crate::grade::process::Backend;
use crate::run::srt::{Sandboxed, Srt};
use crate::run::{RunError, Trial};

pub(crate) trait Sandbox {
    type Backend<'a>: Backend
    where
        Self: 'a;

    /// Runs `trial`'s own commands, its client's included, under its policy.
    fn for_trial(&self, trial: &Trial) -> Result<Self::Backend<'_>, RunError>;

    /// Runs grading of `trial`'s submission under its grading policy.
    fn for_grading(&self, trial: &Trial) -> Result<Self::Backend<'_>, RunError>;
}

impl Sandbox for Srt {
    type Backend<'a> = Sandboxed<'a>;

    fn for_trial(&self, trial: &Trial) -> Result<Sandboxed<'_>, RunError> {
        Srt::for_trial(self, trial)
    }

    fn for_grading(&self, trial: &Trial) -> Result<Sandboxed<'_>, RunError> {
        Srt::for_grading(self, trial)
    }
}
