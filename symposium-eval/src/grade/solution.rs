//! `--check-tasks` imports each task's `solution/` exactly as it would an agent's
//! submission, so a solution cannot pass by changing a file an agent may not contribute.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::ImportError;
use super::import::{Import, build_grading_copy, copy_tree};
use crate::evals::Evals;
use crate::task::{DirProblem, Task, TaskError, TaskErrors};

/// Imports `solution/`, when the task has one, laid over a copy of the starter.
///
/// # Errors
///
/// When `solution/` is not a real directory, holds a link or special file, or changes
/// anything outside `editable`.
pub(crate) fn check_solution(evals: &Evals, task: &Task) -> Result<(), TaskErrors> {
    let Some(solution) = solution_dir(evals, task)? else {
        return Ok(());
    };
    let import = submit(evals, task, &solution).map_err(TaskError::SolutionImport)?;
    let errors: Vec<TaskError> = import
        .outside_editable()
        .iter()
        .cloned()
        .map(TaskError::SolutionOutsideEditable)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.into())
    }
}

/// The task's `solution/`, or `None` when it has none.
fn solution_dir(evals: &Evals, task: &Task) -> Result<Option<PathBuf>, TaskError> {
    let solution = evals.solution_dir(task.name());
    match DirProblem::of(&solution) {
        Ok(None) => Ok(Some(solution)),
        Ok(Some(DirProblem::Missing)) => Ok(None),
        Ok(Some(problem)) => Err(TaskError::SolutionDir(problem)),
        Err(source) => Err(TaskError::Inspect {
            path: solution,
            source,
        }),
    }
}

/// The starter with `solution/` copied over it, imported like a submission.
fn submit(evals: &Evals, task: &Task, solution: &Path) -> Result<Import, ImportError> {
    let submission = TempDir::new().map_err(|source| ImportError::Write {
        path: std::env::temp_dir(),
        source,
    })?;
    copy_tree(
        &evals.starter_dir(task.name()),
        submission.path(),
        "starter",
    )?;
    copy_tree(solution, submission.path(), "solution")?;
    build_grading_copy(evals, task, submission.path())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::grade::testing::TaskFixture;

    fn check(fixture: &TaskFixture) -> Result<(), String> {
        check_solution(fixture.evals(), fixture.task()).map_err(|errors| errors.to_string())
    }

    #[test]
    fn a_task_without_a_solution_passes() {
        let fixture = TaskFixture::new("");

        assert_eq!(check(&fixture), Ok(()));
    }

    #[test]
    fn a_solution_within_editable_passes() {
        let fixture = TaskFixture::new("");
        fs::create_dir_all(fixture.solution().join("src")).unwrap();
        fs::write(fixture.solution().join("src/lib.rs"), "pub fn done() {}").unwrap();

        assert_eq!(check(&fixture), Ok(()));
    }

    #[test]
    fn a_solution_can_turn_a_file_into_a_directory() {
        let fixture = TaskFixture::new("");
        fs::write(fixture.starter().join("src/data"), "").unwrap();
        fs::create_dir_all(fixture.solution().join("src/data")).unwrap();
        fs::write(fixture.solution().join("src/data/table.rs"), "").unwrap();

        assert_eq!(check(&fixture), Ok(()));
    }

    #[test]
    fn a_solution_can_turn_a_directory_into_a_file() {
        let fixture = TaskFixture::new("");
        fs::create_dir_all(fixture.starter().join("src/generated")).unwrap();
        fs::write(fixture.starter().join("src/generated/old.rs"), "").unwrap();
        fs::create_dir_all(fixture.solution().join("src")).unwrap();
        fs::write(fixture.solution().join("src/generated"), "").unwrap();

        assert_eq!(check(&fixture), Ok(()));
    }

    #[test]
    fn a_solution_that_edits_outside_editable_is_rejected() {
        let fixture = TaskFixture::new("");
        fs::create_dir_all(fixture.solution()).unwrap();
        fs::write(fixture.solution().join("README.md"), "# solved").unwrap();
        fs::write(fixture.solution().join("Cargo.toml"), "[package]").unwrap();

        assert_eq!(
            check(&fixture),
            Err([
                "`solution/` modified `Cargo.toml`, outside `editable`; grading ignores it",
                "`solution/` modified `README.md`, outside `editable`; grading ignores it",
            ]
            .join("\n"))
        );
    }

    #[test]
    fn a_solution_that_is_a_file_is_rejected() {
        let fixture = TaskFixture::new("");
        fs::write(fixture.solution(), "").unwrap();

        assert_eq!(
            check(&fixture),
            Err("`solution/` is not a directory".to_owned())
        );
    }
}
