//! Grading: a trusted copy of the task holding only the submission's editable files, then
//! the code checks (RFD reference, "Grading the code").
//!
//! Invariant: only repository-owned code runs outside the `srt` backend. Today that means
//! `--check-tasks` grades each task's `solution/`; an agent's submission is graded once
//! grading runs through `srt`.

mod error;
mod import;
mod solution;

pub(crate) use error::ImportError;
pub(crate) use import::OutsideEdit;
pub(crate) use solution::check_solution;

#[cfg(test)]
pub(crate) mod testing {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::ImportError;
    use super::import::{Import, build_grading_copy, copy_tree};
    use crate::evals::Evals;
    use crate::task::{Task, TaskName};

    pub(crate) const STARTER_MANIFEST: &str =
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

    /// An `evals/` tree with one valid task, `demo`, and a fixture crate, `store`.
    pub(crate) struct TaskFixture {
        _root: TempDir,
        evals: Evals,
        task: Task,
    }

    impl TaskFixture {
        /// `extra_toml` is appended to `demo`'s `task.toml`.
        pub(crate) fn new(extra_toml: &str) -> Self {
            let root = TempDir::new().unwrap();
            let task_dir = root.path().join("tasks/demo");
            let starter = task_dir.join("starter");
            fs::create_dir_all(starter.join("src")).unwrap();
            fs::write(starter.join("Cargo.toml"), STARTER_MANIFEST).unwrap();
            fs::write(starter.join("Cargo.lock"), "# lock").unwrap();
            fs::write(starter.join("README.md"), "# demo").unwrap();
            fs::write(starter.join("src/lib.rs"), "").unwrap();
            fs::write(starter.join("src/old.rs"), "").unwrap();
            fs::create_dir_all(task_dir.join("checks")).unwrap();
            fs::write(task_dir.join("checks/works.rs"), "#[test]\nfn works() {}\n").unwrap();
            let task_toml = format!("kind = \"regression\"\nprompt = \"x\"\n{extra_toml}");
            fs::write(task_dir.join("task.toml"), task_toml).unwrap();
            let store = root.path().join("fixtures/store/src");
            fs::create_dir_all(&store).unwrap();
            fs::write(store.join("lib.rs"), "pub struct Store;").unwrap();

            let evals = Evals::new(root.path());
            let task = Task::load(&evals, &TaskName::new("demo").unwrap()).unwrap();
            Self {
                _root: root,
                evals,
                task,
            }
        }

        pub(crate) fn evals(&self) -> &Evals {
            &self.evals
        }

        pub(crate) fn task(&self) -> &Task {
            &self.task
        }

        pub(crate) fn starter(&self) -> PathBuf {
            self.evals.starter_dir(self.task.name())
        }

        pub(crate) fn solution(&self) -> PathBuf {
            self.evals.solution_dir(self.task.name())
        }

        /// A copy of the starter, as an agent that changed nothing would leave it.
        pub(crate) fn submission(&self) -> TempDir {
            let submission = TempDir::new().unwrap();
            copy_tree(&self.starter(), submission.path(), "starter").unwrap();
            submission
        }

        pub(crate) fn build_copy(&self, submission: &Path) -> Result<Import, ImportError> {
            build_grading_copy(&self.evals, &self.task, submission)
        }
    }
}
