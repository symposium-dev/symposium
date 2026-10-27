//! `--check-tasks` grades each task's `solution/` exactly as it would an agent's
//! submission, so a solution cannot pass by changing a file an agent may not contribute.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::import::copy_tree;
use super::limits::GradeLimits;
use super::process::Local;
use super::{Grading, ImportError, Outcome, Report, grade};
use crate::evals::Evals;
use crate::task::{DirProblem, Task, TaskError, TaskErrors};

/// Grades `solution/`, when the task has one, laid over a copy of the starter.
///
/// # Errors
///
/// When `solution/` is not a real directory, cannot be graded, or changes anything
/// outside `editable`.
pub(crate) fn check_solution(evals: &Evals, task: &Task) -> Result<(), TaskErrors> {
    let Some(solution) = solution_dir(evals, task)? else {
        return Ok(());
    };
    let report = grade_solution(evals, task, &solution)?;
    let errors = report_errors(report);
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

/// Grades `solution/` laid over the starter. Failing to grade a solution, for any reason,
/// is the task's error.
fn grade_solution(evals: &Evals, task: &Task, solution: &Path) -> Result<Report, TaskError> {
    let scratch =
        overlay(evals, task, solution).map_err(|error| TaskError::SolutionGrading(error.into()))?;
    let grading = grade(
        evals,
        task,
        &scratch.path().join("submission"),
        &Local,
        &GradeLimits::default(),
        &scratch.path().join("logs"),
    );
    match grading {
        Grading::Graded(report) => Ok(report),
        Grading::InvalidSubmission(error) => Err(TaskError::SolutionGrading(error.into())),
        Grading::Infrastructure(error) => Err(TaskError::SolutionGrading(error)),
    }
}

/// What a solution's report shows wrong with the task.
fn report_errors(report: Report) -> Vec<TaskError> {
    let edits = report
        .outside_editable
        .into_iter()
        .map(TaskError::SolutionOutsideEditable);
    let failures = report
        .steps
        .into_iter()
        .filter(|step| !matches!(step.outcome, Outcome::Passed | Outcome::Skipped))
        .map(TaskError::SolutionStep);
    edits.chain(failures).collect()
}

/// A scratch directory holding `submission/`, the starter with `solution/` copied over
/// it, and an empty `logs/`.
pub(super) fn overlay(evals: &Evals, task: &Task, solution: &Path) -> Result<TempDir, ImportError> {
    let scratch = TempDir::new().map_err(|source| ImportError::Write {
        path: std::env::temp_dir(),
        source,
    })?;
    let submission = scratch.path().join("submission");
    copy_tree(&evals.starter_dir(task.name()), &submission, "starter")?;
    copy_tree(solution, &submission, "solution")?;
    let logs = scratch.path().join("logs");
    fs::create_dir(&logs).map_err(|source| ImportError::Write { path: logs, source })?;
    Ok(scratch)
}

#[cfg(test)]
mod tests {
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
    fn a_stale_starter_lockfile_is_reported() {
        let fixture = TaskFixture::new("");
        fs::write(fixture.starter().join("Cargo.lock"), "version = 4\n").unwrap();
        fs::create_dir_all(fixture.solution()).unwrap();

        let message = check(&fixture).unwrap_err();

        assert!(
            message.starts_with(
                "cannot grade `solution/`: `cargo metadata` failed on the trusted starter: "
            ),
            "{message}"
        );
    }

    #[test]
    fn a_solution_that_does_not_build_is_rejected() {
        let fixture = TaskFixture::new("");
        fs::create_dir_all(fixture.solution().join("src")).unwrap();
        fs::write(fixture.solution().join("src/lib.rs"), "pub fn broken( {}").unwrap();

        let message = check(&fixture).unwrap_err();

        assert!(
            message.starts_with("`solution/` fails `cargo build`:"),
            "{message}"
        );
        assert!(message.contains("error"), "{message}");
        assert!(!message.contains("skips"), "{message}");
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
