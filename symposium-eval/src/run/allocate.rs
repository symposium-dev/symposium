//! Making a trial in two steps: its root first, so a result always has a place to go,
//! then its contents.
//!
//! Only a [`Trial`] can be set up, launched or graded, and only [`NewTrial::prepare`]
//! makes one, so a trial whose preparation failed can never run. It keeps its root, so
//! the runner can still record why it stopped.

use std::path::{Path, PathBuf};

use super::{Batch, Configuration, Inputs, RunError, Trial, create_dirs, create_fresh};
use crate::grade::{PROJECT_SCAFFOLD, copy_tree};

/// A trial whose root exists and holds nothing yet, with the inputs it will be filled
/// from.
#[derive(Debug)]
#[must_use = "an allocated trial's root stays on disk; prepare it or record why it stopped"]
pub(crate) struct NewTrial<'a> {
    trial: Trial,
    inputs: &'a Inputs,
}

/// A trial whose preparation failed, with the root it keeps for its result.
#[derive(Debug)]
pub(crate) struct NotPrepared {
    root: PathBuf,
    error: RunError,
}

impl Batch {
    /// Creates trial `number`'s root, empty, to be filled from `inputs`, this batch's
    /// copy of the task.
    ///
    /// # Errors
    ///
    /// When `inputs` are another batch's, which never gets a root, or the trial already
    /// exists, so a restarted run never inherits an earlier attempt, or its root cannot
    /// be created.
    pub(crate) fn allocate_trial<'a>(
        &self,
        number: usize,
        configuration: Configuration,
        inputs: &'a Inputs,
    ) -> Result<NewTrial<'a>, RunError> {
        if inputs.batch() != self.root {
            return Err(RunError::ForeignInputs(inputs.batch().to_owned()));
        }
        let trial = Trial {
            batch: self.clone(),
            root: self.root.join(format!("trial-{number}-{configuration}")),
            configuration,
        };
        create_fresh(&trial.root)?;
        Ok(NewTrial { trial, inputs })
    }

    /// Allocates trial `number` and prepares it from `inputs`.
    ///
    /// # Errors
    ///
    /// As [`Self::allocate_trial`] and [`NewTrial::prepare`].
    pub(crate) fn create_trial(
        &self,
        number: usize,
        configuration: Configuration,
        inputs: &Inputs,
    ) -> Result<Trial, RunError> {
        self.allocate_trial(number, configuration, inputs)?
            .prepare()
            .map_err(NotPrepared::into_error)
    }
}

impl NewTrial<'_> {
    pub(crate) fn root(&self) -> &Path {
        self.trial.root()
    }

    /// Fills the trial: empty working directories, the starter in `project/`, the
    /// fixtures beside it, and, for Symposium, the task's plugins as the trial's
    /// registry.
    ///
    /// # Errors
    ///
    /// When a directory cannot be created or the task cannot be copied. The root is
    /// kept.
    pub(crate) fn prepare(self) -> Result<Trial, NotPrepared> {
        match self.fill() {
            Ok(()) => Ok(self.trial),
            Err(error) => Err(NotPrepared {
                root: self.trial.root,
                error,
            }),
        }
    }

    fn fill(&self) -> Result<(), RunError> {
        let trial = &self.trial;
        let (evals, task) = (self.inputs.evals(), self.inputs.task());
        create_dirs([
            trial.home(),
            trial.bin(),
            trial.cargo_home(),
            trial.tmp(),
            trial.evidence(),
            trial.session(),
        ])?;
        trial.batch.link_tools(trial)?;
        copy_tree(&evals.starter_dir(task.name()), &trial.project(), "starter")
            .map_err(RunError::Copy)?;
        create_dirs(PROJECT_SCAFFOLD.map(|dir| trial.project().join(dir)))?;
        // Fixtures are not in the policy's writable paths, so `srt` keeps them read-only.
        for fixture in task.fixtures() {
            let destination = trial.fixtures().join(fixture.as_str());
            copy_tree(&evals.fixture_dir(fixture), &destination, "fixture")
                .map_err(RunError::Copy)?;
        }
        if trial.configuration == Configuration::Symposium {
            create_dirs([trial.registry()])?;
            for plugin in task.plugins() {
                let destination = trial.registry().join(plugin.as_str());
                copy_tree(&evals.plugin_dir(plugin), &destination, "plugin")
                    .map_err(RunError::Copy)?;
            }
        }
        Ok(())
    }
}

impl NotPrepared {
    /// The root the trial was allocated, where its result goes.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn error(&self) -> &RunError {
        &self.error
    }

    pub(crate) fn into_error(self) -> RunError {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use tempfile::TempDir;

    use super::*;
    use crate::run::test_support::store_durability;
    use crate::task::BatchName;

    fn batch(root: &TempDir) -> Batch {
        Batch::create(root.path(), &BatchName::new("batch").unwrap()).unwrap()
    }

    #[test]
    fn allocating_a_trial_creates_only_its_empty_root() {
        let root = TempDir::new().unwrap();
        let batch = batch(&root);
        let inputs = store_durability(&batch);

        let new = batch
            .allocate_trial(1, Configuration::Symposium, &inputs)
            .unwrap();

        assert_eq!(
            new.root(),
            batch.root.join("trial-1-with-symposium").as_path()
        );
        assert_eq!(fs::read_dir(new.root()).unwrap().count(), 0);
    }

    #[test]
    fn an_allocated_root_is_never_allocated_again() {
        let root = TempDir::new().unwrap();
        let batch = batch(&root);
        let inputs = store_durability(&batch);
        let _first = batch
            .allocate_trial(1, Configuration::Baseline, &inputs)
            .unwrap();

        let again = batch.allocate_trial(1, Configuration::Baseline, &inputs);

        assert!(
            matches!(&again, Err(RunError::Create { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists),
            "{again:?}"
        );
    }

    #[test]
    fn a_failed_preparation_keeps_its_root() {
        let root = TempDir::new().unwrap();
        let batch = batch(&root);
        let inputs = store_durability(&batch);
        // The batch's own copy of the task, broken after it was taken.
        fs::remove_dir_all(inputs.evals().starter_dir(inputs.task().name())).unwrap();
        let new = batch
            .allocate_trial(1, Configuration::Symposium, &inputs)
            .unwrap();
        let allocated = new.root().to_owned();

        let failed = new.prepare().unwrap_err();

        assert_eq!(failed.root(), allocated);
        assert!(
            matches!(failed.error(), RunError::Copy(_)),
            "{:?}",
            failed.error()
        );
        assert!(allocated.is_dir());
    }

    #[test]
    fn preparing_fills_the_allocated_root() {
        let root = TempDir::new().unwrap();
        let batch = batch(&root);
        let inputs = store_durability(&batch);
        let new = batch
            .allocate_trial(1, Configuration::Baseline, &inputs)
            .unwrap();
        let allocated = new.root().to_owned();

        let trial = new.prepare().unwrap();

        assert_eq!(trial.root(), allocated);
        assert!(trial.project().join("Cargo.lock").is_file());
        assert!(trial.project().join(".claude").is_dir());
    }
}
