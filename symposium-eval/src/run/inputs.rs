//! The batch's own copy of a task, its fixtures and its plugins, taken once (RFD
//! reference, "Preparing a trial").
//!
//! Every trial is made from it, and the result's content hash is its digest, so a
//! change to `evals/` during a batch reaches no trial, and the hash names what every
//! trial ran. [`Batch::create_trial`] takes only an [`Inputs`], and only its own batch's,
//! so no trial can be made from anything else. It sits in the batch directory, which
//! the policy keeps every trial from reading, since it holds the hidden checks.

use std::path::{Path, PathBuf};

use super::{Batch, RunError, create_dirs, create_fresh};
use crate::digest::{self, Digest};
use crate::evals::Evals;
use crate::grade::copy_exact;
use crate::task::{Task, TaskName};

/// A task as one batch copied it.
#[derive(Debug)]
pub(crate) struct Inputs {
    /// The batch it belongs to.
    batch: PathBuf,
    evals: Evals,
    task: Task,
    digest: Digest,
}

impl Inputs {
    /// The copy, laid out as `evals/` is, for trials and grading to read.
    pub(crate) fn evals(&self) -> &Evals {
        &self.evals
    }

    /// The task, loaded and checked from the copy.
    pub(crate) fn task(&self) -> &Task {
        &self.task
    }

    /// The digest of the copy, `inputs/` as one tree: the result's content hash.
    pub(crate) fn digest(&self) -> Digest {
        self.digest
    }

    /// The batch these inputs belong to.
    pub(super) fn batch(&self) -> &Path {
        &self.batch
    }

    /// Loads and hashes what `batch` copied for task `name`.
    fn read(batch: &Batch, name: &TaskName) -> Result<Self, RunError> {
        let evals = Evals::new(batch.inputs());
        // Loaded from the copy, so the task every trial runs is the one copied, even if
        // `task.toml` changed meanwhile.
        let task = Task::load(&evals, name).map_err(RunError::Task)?;
        let digest = digest::tree(&batch.inputs()).map_err(RunError::Digest)?;
        Ok(Self {
            batch: batch.root.clone(),
            evals,
            task,
            digest,
        })
    }

    /// The inputs `batch` already copied for task `name`, so a test can create trials
    /// in a batch it did not snapshot itself.
    #[cfg(test)]
    pub(crate) fn reopen(batch: &Batch, name: &TaskName) -> Result<Self, RunError> {
        Self::read(batch, name)
    }
}

impl Batch {
    /// Where the batch keeps its copy of the task.
    pub(crate) fn inputs(&self) -> PathBuf {
        self.root.join("inputs")
    }

    /// Copies task `name` from `source`, with its fixtures and plugins, into `inputs/`,
    /// laid out as `evals/` is, then loads and hashes the copy. Taken once per batch.
    ///
    /// # Errors
    ///
    /// When the task is invalid in `source` or in the copy, the batch already has a
    /// copy, or the copy cannot be made or hashed.
    pub(crate) fn snapshot(&self, source: &Evals, name: &TaskName) -> Result<Inputs, RunError> {
        let named = Task::load(source, name).map_err(RunError::Task)?;
        create_fresh(&self.inputs())?;
        let evals = Evals::new(self.inputs());
        copy(&source.task_dir(name), &evals.task_dir(name))?;
        for fixture in named.fixtures() {
            copy(&source.fixture_dir(fixture), &evals.fixture_dir(fixture))?;
        }
        for plugin in named.plugins() {
            copy(&source.plugin_dir(plugin), &evals.plugin_dir(plugin))?;
        }
        Inputs::read(self, name)
    }
}

/// Copies the tree at `source` exactly, so the copy is what was checked in.
fn copy(source: &Path, destination: &Path) -> Result<(), RunError> {
    if let Some(parent) = destination.parent() {
        create_dirs([parent.to_owned()])?;
    }
    copy_exact(source, destination, "task input").map_err(RunError::Snapshot)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::run::Configuration;
    use crate::run::test_support::Host;
    use crate::task::SkillName;

    /// A copy of the repository's `evals/` to change, with the `formatting` task.
    fn source() -> (TempDir, Evals) {
        let root = TempDir::new().unwrap();
        let repository = Evals::in_repository();
        let evals = Evals::new(root.path());
        let name = formatting();
        let task = Task::load(&repository, &name).unwrap();
        copy(&repository.task_dir(&name), &evals.task_dir(&name)).unwrap();
        for plugin in task.plugins() {
            copy(&repository.plugin_dir(plugin), &evals.plugin_dir(plugin)).unwrap();
        }
        (root, evals)
    }

    fn formatting() -> TaskName {
        TaskName::new("formatting").unwrap()
    }

    #[test]
    fn a_change_to_evals_after_the_snapshot_reaches_no_trial() {
        let (_root, source) = source();
        let host = Host::new();
        let batch = host.batch();
        let inputs = batch.snapshot(&source, &formatting()).unwrap();
        let starter = source.starter_dir(&formatting()).join("src/lib.rs");
        let original = fs::read_to_string(&starter).unwrap();

        fs::write(&starter, "// changed during the batch\n").unwrap();
        let trial = batch
            .create_trial(1, Configuration::Symposium, &inputs)
            .unwrap();

        let copied = fs::read_to_string(trial.project().join("src/lib.rs")).unwrap();
        assert_eq!(copied, original);
        assert_eq!(inputs.digest(), digest::tree(&batch.inputs()).unwrap());
    }

    #[test]
    fn the_task_is_loaded_from_the_copy() {
        let (_root, source) = source();
        let host = Host::new();
        let batch = host.batch();

        let inputs = batch.snapshot(&source, &formatting()).unwrap();

        let skill = SkillName::new("rust-formatting").unwrap();
        let provider = inputs.task().skill_source(&skill).unwrap();
        assert!(
            provider.starts_with(batch.inputs()),
            "{}",
            provider.display()
        );
    }

    #[test]
    fn a_batch_takes_one_snapshot() {
        let (_root, source) = source();
        let host = Host::new();
        let batch = host.batch();
        batch.snapshot(&source, &formatting()).unwrap();

        let again = batch.snapshot(&source, &formatting());

        assert!(matches!(again, Err(RunError::Create { .. })), "{again:?}");
    }

    #[test]
    fn a_trial_is_never_made_from_another_batchs_inputs() {
        let (_root, source) = source();
        let host = Host::new();
        let first = host.batch();
        let second = Batch::create(
            &host.join("eval"),
            &crate::task::BatchName::new("second").unwrap(),
        )
        .unwrap();
        let inputs = first.snapshot(&source, &formatting()).unwrap();

        let trial = second.create_trial(1, Configuration::Symposium, &inputs);

        assert!(
            matches!(trial, Err(RunError::ForeignInputs(_))),
            "{trial:?}"
        );
        assert!(!second.root.join("trial-1-with-symposium").exists());
    }
}
