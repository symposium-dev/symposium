//! Grading: a trusted copy of the task holding only the submission's editable files, then
//! the code checks (RFD reference, "Grading the code").
//!
//! Invariant: only repository-owned code runs outside the `srt` backend. Today that means
//! `--check-tasks` grades each task's `solution/` with [`process::Local`]; an agent's
//! submission is graded once grading runs through `srt`.

mod cargo;
mod error;
mod import;
mod limits;
mod process;
mod solution;

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) use error::{GradeError, ImportError, ProcessError};
pub(crate) use import::OutsideEdit;
pub(crate) use solution::check_solution;

use crate::evals::Evals;
use crate::task::Task;
use cargo::Cargo;
use import::{GradingCopy, import_submission};
use limits::{Budget, GradeLimits};
use process::{Backend, Finished, Invocation};

/// What grading concluded about one submission.
#[derive(Debug)]
pub(crate) enum Grading {
    Graded(Report),
    /// The submission cannot be graded, which fails the trial.
    InvalidSubmission(ImportError),
    /// Grading itself failed, which excludes the trial; its submission can be graded
    /// again.
    Infrastructure(GradeError),
}

#[derive(Debug)]
pub(crate) struct Report {
    /// Sorted by path.
    pub(crate) outside_editable: Vec<OutsideEdit>,
    /// The checks run on the submission, in order.
    pub(crate) steps: Vec<Step>,
}

/// One check run on the submission.
#[derive(Debug)]
pub(crate) struct Step {
    /// The Cargo subcommand, such as `build`.
    pub(crate) command: &'static str,
    pub(crate) outcome: Outcome,
}

/// How a check went. Every outcome here is the submission's: a command that could not be
/// run at all is a [`GradeError`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Passed,
    Failed {
        /// The end of the command's error output.
        stderr: String,
        /// Whether earlier error output was left out of `stderr`; the log has it all.
        truncated: bool,
    },
    TimedOut,
}

/// How much of a failed step's error output a message shows; the log has all of it.
const SHOWN_LINES: usize = 20;

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let command = self.command;
        match &self.outcome {
            Outcome::Passed => write!(f, "passes `cargo {command}`"),
            Outcome::TimedOut => write!(f, "did not finish `cargo {command}` in time"),
            Outcome::Failed { stderr, .. } => {
                write!(f, "fails `cargo {command}`:")?;
                let lines: Vec<&str> = stderr.trim_end().lines().collect();
                for line in &lines[lines.len().saturating_sub(SHOWN_LINES)..] {
                    write!(f, "\n{line}")?;
                }
                Ok(())
            }
        }
    }
}

/// Grades `submission` against `task`, running commands through `backend` and writing
/// their output to log files in `logs`.
pub(crate) fn grade(
    evals: &Evals,
    task: &Task,
    submission: &Path,
    backend: &impl Backend,
    limits: &GradeLimits,
    logs: &Path,
) -> Grading {
    match try_grade(evals, task, submission, backend, limits, logs) {
        Ok(report) => Grading::Graded(report),
        Err(GradeError::Import(error)) if error.invalidates_submission() => {
            Grading::InvalidSubmission(error)
        }
        Err(error) => Grading::Infrastructure(error),
    }
}

fn try_grade(
    evals: &Evals,
    task: &Task,
    submission: &Path,
    backend: &impl Backend,
    limits: &GradeLimits,
    logs: &Path,
) -> Result<Report, GradeError> {
    let mut commands = Commands {
        backend,
        budget: Budget::new(limits.commands),
        logs,
    };
    let copy = GradingCopy::from_trusted(evals, task)?;
    let cargo = Cargo::new(copy.project(), copy.scratch());
    let build_scripts =
        read_build_scripts(&cargo, &copy.project(), &mut commands, limits.metadata)?;
    let outside_editable = import_submission(&copy, task.editable(), &build_scripts, submission)?;
    let build = run_step("build", &cargo.build(), &mut commands, limits.build)?;
    Ok(Report {
        outside_editable,
        steps: vec![build],
    })
}

/// Runs grading's commands through one backend, charging each to the command budget.
struct Commands<'a, B> {
    backend: &'a B,
    budget: Budget,
    logs: &'a Path,
}

impl<B: Backend> Commands<'_, B> {
    /// Runs `invocation` for at most `cap`, or what is left of the budget if that is less.
    /// With no time left, the command is not started at all, so a spent budget cannot keep
    /// launching and stopping commands.
    fn run(&mut self, invocation: &Invocation, cap: Duration) -> Result<Finished, ProcessError> {
        let allowance = self.budget.allowance(cap);
        if allowance.is_zero() {
            return Ok(Finished::never_started());
        }
        let started = Instant::now();
        let finished = self.backend.run(invocation, allowance, self.logs);
        self.budget.charge(started.elapsed());
        finished
    }
}

/// Runs one check on the imported submission. A failure or deadline Cargo reports is the
/// submission's outcome; a failure to start, supervise, stop, or collect the command is
/// an infrastructure error.
fn run_step(
    command: &'static str,
    invocation: &Invocation,
    commands: &mut Commands<'_, impl Backend>,
    cap: Duration,
) -> Result<Step, GradeError> {
    let finished = commands
        .run(invocation, cap)
        .map_err(|source| GradeError::Process {
            step: command,
            source,
        })?;
    let outcome = if finished.timed_out() {
        Outcome::TimedOut
    } else if finished.succeeded() {
        Outcome::Passed
    } else {
        Outcome::Failed {
            stderr: finished.stderr.tail(),
            truncated: finished.stderr.truncated(),
        }
    };
    Ok(Step { command, outcome })
}

/// Resolves the trusted starter against its lockfile and finds its build scripts at
/// custom paths, before anything from the submission is in the copy. A failure here is
/// the task's or the machine's: the submission has not been imported yet.
fn read_build_scripts(
    cargo: &Cargo,
    project: &Path,
    commands: &mut Commands<'_, impl Backend>,
    cap: Duration,
) -> Result<BTreeSet<String>, GradeError> {
    const STEP: &str = "metadata";
    let finished = commands
        .run(&cargo.metadata(), cap)
        .map_err(|source| GradeError::Process { step: STEP, source })?;
    if finished.timed_out() {
        return Err(GradeError::TimedOut { step: STEP });
    }
    if !finished.succeeded() {
        return Err(GradeError::Preflight {
            step: STEP,
            stderr: finished.stderr.tail().trim().to_owned(),
        });
    }
    cargo::build_scripts(finished.stdout.log(), project)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::grade::process::Local;
    use crate::grade::testing::TaskFixture;

    fn grade_with(fixture: &TaskFixture, lib_rs: &str, limits: &GradeLimits) -> Report {
        let submission = fixture.submission();
        fs::write(submission.path().join("src/lib.rs"), lib_rs).unwrap();
        let logs = TempDir::new().unwrap();
        match grade(
            fixture.evals(),
            fixture.task(),
            submission.path(),
            &Local,
            limits,
            logs.path(),
        ) {
            Grading::Graded(report) => report,
            other => panic!("expected a report, got {other:?}"),
        }
    }

    /// Records each allowance it is given, then spends `running` as if a command ran.
    struct Recording {
        allowances: RefCell<Vec<Duration>>,
        running: Duration,
    }

    impl Backend for Recording {
        fn run(
            &self,
            _invocation: &Invocation,
            deadline: Duration,
            _logs: &Path,
        ) -> Result<Finished, ProcessError> {
            self.allowances.borrow_mut().push(deadline);
            thread::sleep(self.running);
            Err(ProcessError::OutputNotClosed)
        }
    }

    #[test]
    fn time_between_commands_is_not_charged() {
        let backend = Recording {
            allowances: RefCell::new(Vec::new()),
            running: Duration::from_millis(100),
        };
        let logs = TempDir::new().unwrap();
        let mut commands = Commands {
            backend: &backend,
            budget: Budget::new(Duration::from_secs(10)),
            logs: logs.path(),
        };
        let invocation = Cargo::new(logs.path().to_owned(), logs.path()).build();

        let _ = commands.run(&invocation, Duration::from_secs(60));
        // Preparing files between commands, such as importing the submission. The gap is
        // far longer than the command, so a slow machine cannot blur the two.
        thread::sleep(Duration::from_secs(3));
        let _ = commands.run(&invocation, Duration::from_secs(60));

        let allowances = backend.allowances.borrow();
        assert_eq!(allowances[0], Duration::from_secs(10));
        let charged = Duration::from_secs(10).saturating_sub(allowances[1]);
        assert!(
            charged >= Duration::from_millis(100) && charged < Duration::from_secs(2),
            "{charged:?}"
        );
    }

    #[test]
    fn a_spent_budget_starts_no_command() {
        let backend = Recording {
            allowances: RefCell::new(Vec::new()),
            running: Duration::ZERO,
        };
        let logs = TempDir::new().unwrap();
        let mut commands = Commands {
            backend: &backend,
            budget: Budget::new(Duration::ZERO),
            logs: logs.path(),
        };
        let invocation = Cargo::new(logs.path().to_owned(), logs.path()).build();

        let finished = commands.run(&invocation, Duration::from_secs(60)).unwrap();

        assert!(finished.timed_out());
        assert!(backend.allowances.borrow().is_empty());
    }

    #[test]
    fn a_submission_that_builds_passes_the_build_step() {
        let fixture = TaskFixture::new("");

        let report = grade_with(&fixture, "pub fn works() {}", &GradeLimits::default());

        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].outcome, Outcome::Passed);
    }

    #[test]
    fn a_build_failure_is_the_submissions_outcome() {
        let fixture = TaskFixture::new("");

        let report = grade_with(&fixture, "pub fn broken( {}", &GradeLimits::default());

        assert!(
            matches!(&report.steps[0].outcome, Outcome::Failed { stderr, .. } if stderr.contains("error")),
            "{report:?}"
        );
    }

    #[test]
    fn a_build_past_its_deadline_times_out() {
        let fixture = TaskFixture::new("");
        let limits = GradeLimits {
            build: Duration::ZERO,
            ..GradeLimits::default()
        };

        let report = grade_with(&fixture, "pub fn works() {}", &limits);

        assert_eq!(report.steps[0].outcome, Outcome::TimedOut);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::ImportError;
    use super::import::{GradingCopy, OutsideEdit, copy_tree, import_submission};
    use crate::evals::Evals;
    use crate::task::{Task, TaskName};

    pub(crate) const STARTER_MANIFEST: &str =
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

    /// What `cargo generate-lockfile` writes for [`STARTER_MANIFEST`].
    const STARTER_LOCKFILE: &str = "# This file is automatically @generated by Cargo.\n\
        # It is not intended for manual editing.\n\
        version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";

    /// An `evals/` tree with one valid task, `demo`, and a fixture crate, `store`.
    pub(crate) struct TaskFixture {
        _root: TempDir,
        evals: Evals,
        task: Task,
    }

    /// A grading copy with a submission imported, and what it changed outside `editable`.
    #[derive(Debug)]
    pub(crate) struct Imported {
        pub(crate) copy: GradingCopy,
        edits: Vec<OutsideEdit>,
    }

    impl TaskFixture {
        /// `extra_toml` is appended to `demo`'s `task.toml`.
        pub(crate) fn new(extra_toml: &str) -> Self {
            let root = TempDir::new().unwrap();
            let task_dir = root.path().join("tasks/demo");
            let starter = task_dir.join("starter");
            fs::create_dir_all(starter.join("src")).unwrap();
            fs::write(starter.join("Cargo.toml"), STARTER_MANIFEST).unwrap();
            fs::write(starter.join("Cargo.lock"), STARTER_LOCKFILE).unwrap();
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

        pub(crate) fn build_copy(&self, submission: &Path) -> Result<Imported, ImportError> {
            self.build_copy_with(submission, &BTreeSet::new())
        }

        pub(crate) fn build_copy_with(
            &self,
            submission: &Path,
            build_scripts: &BTreeSet<String>,
        ) -> Result<Imported, ImportError> {
            let copy = GradingCopy::from_trusted(&self.evals, &self.task)?;
            let edits = import_submission(&copy, self.task.editable(), build_scripts, submission)?;
            Ok(Imported { copy, edits })
        }
    }

    impl Imported {
        pub(crate) fn outside_editable(&self) -> &[OutsideEdit] {
            &self.edits
        }
    }
}
