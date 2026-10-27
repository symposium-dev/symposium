//! Grading: a trusted copy of the task holding only the submission's editable files, then
//! the code checks (RFD reference, "Grading the code").
//!
//! Invariant: only repository-owned code runs outside the `srt` backend. Today that means
//! `--check-tasks` grades each task's `solution/` with [`process::Local`]; an agent's
//! submission is graded once grading runs through `srt`.

mod cargo;
mod error;
mod hidden;
mod import;
mod limits;
mod process;
#[cfg(test)]
mod repository_tasks;
mod solution;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) use error::{GradeError, ImportError, ListingProblem, ProcessError};
pub(crate) use import::OutsideEdit;
pub(crate) use solution::check_solution;

use crate::evals::Evals;
use crate::task::{CheckName, Task};
use cargo::{Cargo, StarterTargets};
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
    pub(crate) kind: StepKind,
    pub(crate) outcome: Outcome,
}

/// Which check a step is, in the order grading runs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepKind {
    Build,
    TestLib,
    /// One hidden check, run as exactly one test.
    Hidden(CheckName),
    Fmt,
}

/// How a check went. Every outcome here is the submission's: a command that could not be
/// run at all is a [`GradeError`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Passed,
    Failed {
        /// The end of the command's output; a failing test reports on stdout.
        stdout: String,
        stderr: String,
        /// Whether earlier output was left out of either; the logs have it all.
        truncated: bool,
    },
    TimedOut,
    /// Exited successfully, but its output did not confirm exactly one passing test.
    Unconfirmed {
        stdout: String,
        stderr: String,
        /// Whether earlier output was left out of either; the logs have it all.
        truncated: bool,
    },
    /// Not run, because the build failed.
    Skipped,
}

impl StepKind {
    /// The command line the step runs, for messages.
    pub(crate) fn command(&self) -> String {
        match self {
            Self::Build => "cargo build".to_owned(),
            Self::TestLib => "cargo test --lib".to_owned(),
            Self::Hidden(check) => format!("cargo test --test {check}"),
            Self::Fmt => "cargo fmt --check".to_owned(),
        }
    }
}

impl fmt::Display for StepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hidden(check) => write!(f, "hidden check `{check}`"),
            other => write!(f, "`{}`", other.command()),
        }
    }
}

/// How much of a failed step's output a message shows; the logs have all of it.
const SHOWN_LINES: usize = 20;

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = &self.kind;
        let output = match &self.outcome {
            Outcome::Passed => return write!(f, "passes {kind}"),
            Outcome::TimedOut => return write!(f, "did not finish {kind} in time"),
            Outcome::Skipped => return write!(f, "skips {kind} after the build failed"),
            Outcome::Failed { stdout, stderr, .. } => {
                write!(f, "fails {kind}:")?;
                [stdout.as_str(), stderr.as_str()]
            }
            Outcome::Unconfirmed { stdout, stderr, .. } => {
                write!(
                    f,
                    "exits successfully from {kind}, but its output does not confirm one passing test:"
                )?;
                [stdout.as_str(), stderr.as_str()]
            }
        };
        let lines: Vec<&str> = output.iter().flat_map(|text| text.lines()).collect();
        for line in &lines[lines.len().saturating_sub(SHOWN_LINES)..] {
            write!(f, "\n{line}")?;
        }
        Ok(())
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
    let prepared = prepare(evals, task, &mut commands, limits)?;
    let outside_editable = import_submission(
        &prepared.copy,
        task.editable(),
        &prepared.starter_owned,
        submission,
    )?;
    let steps = run_steps(task, &prepared.cargo, &mut commands, limits)?;
    Ok(Report {
        outside_editable,
        steps,
    })
}

/// A grading copy of the trusted starter, checked before any submission reaches it.
struct Prepared {
    copy: GradingCopy,
    cargo: Cargo,
    /// Paths the import keeps the starter's beyond the protected names.
    starter_owned: BTreeSet<String>,
}

/// Copies the trusted starter and checks it, and the machine, before the submission is
/// imported, so a failure here is never the submission's.
fn prepare(
    evals: &Evals,
    task: &Task,
    commands: &mut Commands<'_, impl Backend>,
    limits: &GradeLimits,
) -> Result<Prepared, GradeError> {
    let copy = GradingCopy::from_trusted(evals, task)?;
    let cargo = Cargo::new(copy.project(), copy.scratch());
    let starter = inspect_starter(&cargo, "metadata", &copy, commands, limits.metadata)?;
    place_checks(evals, task, &copy, &starter.tests)?;
    let placed = inspect_starter(&cargo, "metadata-checks", &copy, commands, limits.metadata)?;
    verify_check_targets(task.checks(), &placed.tests)?;
    if task.fmt_check() {
        preflight(
            commands,
            &cargo.fmt_version(),
            "cargo fmt --version",
            limits.fmt,
        )?;
    }
    Ok(Prepared {
        copy,
        cargo,
        starter_owned: starter_owned(starter.build_scripts, task.checks()),
    })
}

/// Places each hidden check in the copy at `tests/<check>.rs`, once the starter is shown
/// not to declare a test of that name itself.
fn place_checks(
    evals: &Evals,
    task: &Task,
    copy: &GradingCopy,
    declared: &BTreeMap<String, Vec<String>>,
) -> Result<(), GradeError> {
    reject_declared_checks(task.checks(), declared)?;
    for check in task.checks() {
        copy.place_check(&evals.check_file(task.name(), check), check)?;
    }
    Ok(())
}

/// The paths the import keeps the starter's beyond the protected names: build scripts at
/// custom paths, and the placed hidden checks.
fn starter_owned(build_scripts: BTreeSet<String>, checks: &[CheckName]) -> BTreeSet<String> {
    let mut owned = build_scripts;
    owned.extend(checks.iter().map(|check| format!("tests/{check}.rs")));
    owned
}

/// Builds the imported submission, then runs each later step, or skips it when the build
/// failed.
fn run_steps(
    task: &Task,
    cargo: &Cargo,
    commands: &mut Commands<'_, impl Backend>,
    limits: &GradeLimits,
) -> Result<Vec<Step>, GradeError> {
    let build = run_step(StepKind::Build, &cargo.build(), commands, limits.build)?;
    let build_passed = build.outcome == Outcome::Passed;
    let mut later = vec![StepKind::TestLib];
    later.extend(task.checks().iter().cloned().map(StepKind::Hidden));
    if task.fmt_check() {
        later.push(StepKind::Fmt);
    }
    let mut steps = vec![build];
    for kind in later {
        steps.push(match kind {
            _ if !build_passed => Step {
                kind,
                outcome: Outcome::Skipped,
            },
            StepKind::Hidden(check) => hidden::run(cargo, commands, &check, limits.test)?,
            StepKind::TestLib => run_step(kind, &cargo.test_lib(), commands, limits.test)?,
            StepKind::Fmt => run_step(kind, &cargo.fmt_check(), commands, limits.fmt)?,
            StepKind::Build => unreachable!("the build runs first"),
        });
    }
    Ok(steps)
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
    kind: StepKind,
    invocation: &Invocation,
    commands: &mut Commands<'_, impl Backend>,
    cap: Duration,
) -> Result<Step, GradeError> {
    let finished = commands
        .run(invocation, cap)
        .map_err(|source| GradeError::Process {
            step: kind.command(),
            source,
        })?;
    Ok(Step {
        kind,
        outcome: outcome_of(&finished),
    })
}

/// The submission's outcome for a command that ran.
fn outcome_of(finished: &Finished) -> Outcome {
    if finished.timed_out() {
        Outcome::TimedOut
    } else if finished.succeeded() {
        Outcome::Passed
    } else {
        Outcome::Failed {
            stdout: finished.stdout.tail(),
            stderr: finished.stderr.tail(),
            truncated: finished.stdout.truncated() || finished.stderr.truncated(),
        }
    }
}

/// Resolves the trusted starter against its lockfile and finds its build scripts at
/// custom paths, before anything from the submission is in the copy. A failure here is
/// the task's or the machine's: the submission has not been imported yet.
fn inspect_starter(
    cargo: &Cargo,
    log_name: &str,
    copy: &GradingCopy,
    commands: &mut Commands<'_, impl Backend>,
    cap: Duration,
) -> Result<StarterTargets, GradeError> {
    let finished = preflight(commands, &cargo.metadata(log_name), "cargo metadata", cap)?;
    cargo::starter_targets(finished.stdout.log(), &copy.project())
}

/// Before the checks are placed, no test target may carry a check's name. Metadata does
/// not report `harness`, so a declared target with `harness = false` would look the same
/// as the placed check afterwards, and its test would never run under libtest.
fn reject_declared_checks(
    checks: &[CheckName],
    tests: &BTreeMap<String, Vec<String>>,
) -> Result<(), GradeError> {
    match checks
        .iter()
        .find(|check| tests.contains_key(check.as_str()))
    {
        Some(check) => Err(GradeError::CheckDeclared {
            check: check.clone(),
        }),
        None => Ok(()),
    }
}

/// Each hidden check must be the only test target named after it, built from the file
/// grading placed. A manifest can turn off test discovery or point a target elsewhere,
/// and `cargo test --test <check>` would then run some other test.
fn verify_check_targets(
    checks: &[CheckName],
    tests: &BTreeMap<String, Vec<String>>,
) -> Result<(), GradeError> {
    for check in checks {
        let placed = format!("tests/{check}.rs");
        let found = tests.get(check.as_str()).map_or(&[][..], Vec::as_slice);
        if found != [placed] {
            return Err(GradeError::CheckTarget {
                check: check.clone(),
                found: found.to_vec(),
            });
        }
    }
    Ok(())
}

/// Runs a command that must succeed on the trusted starter. Its failure is the task's or
/// the machine's, never the submission's.
fn preflight(
    commands: &mut Commands<'_, impl Backend>,
    invocation: &Invocation,
    step: &str,
    cap: Duration,
) -> Result<Finished, GradeError> {
    let finished = commands
        .run(invocation, cap)
        .map_err(|source| GradeError::Process {
            step: step.to_owned(),
            source,
        })?;
    if finished.timed_out() {
        return Err(GradeError::TimedOut {
            step: step.to_owned(),
        });
    }
    if !finished.succeeded() {
        return Err(GradeError::Preflight {
            step: step.to_owned(),
            stderr: finished.stderr.tail().trim().to_owned(),
        });
    }
    Ok(finished)
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
    use crate::grade::testing::{STARTER_MANIFEST, TaskFixture};

    fn grade(fixture: &TaskFixture, lib_rs: &str) -> Report {
        grade_with(fixture, lib_rs, &GradeLimits::default())
    }

    fn grade_with(fixture: &TaskFixture, lib_rs: &str, limits: &GradeLimits) -> Report {
        let submission = fixture.submission();
        fs::write(submission.path().join("src/lib.rs"), lib_rs).unwrap();
        let logs = TempDir::new().unwrap();
        match super::grade(
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

        let report = grade(&fixture, "pub fn works() {}");

        let outcomes: Vec<&Outcome> = report.steps.iter().map(|step| &step.outcome).collect();
        assert_eq!(outcomes, [&Outcome::Passed; 3]);
    }

    #[test]
    fn a_build_failure_is_the_submissions_outcome() {
        let fixture = TaskFixture::new("");

        let report = grade(&fixture, "pub fn broken( {}");

        assert!(
            matches!(&report.steps[0].outcome, Outcome::Failed { stderr, .. } if stderr.contains("error")),
            "{report:?}"
        );
    }

    #[test]
    fn a_build_failure_skips_the_later_steps() {
        let fixture = TaskFixture::new("fmt_check = true\n");

        let report = grade(&fixture, "pub fn broken( {}");

        let outcomes: Vec<&Outcome> = report.steps.iter().map(|step| &step.outcome).collect();
        assert!(matches!(
            outcomes[..],
            [
                Outcome::Failed { .. },
                Outcome::Skipped,
                Outcome::Skipped,
                Outcome::Skipped
            ]
        ));
    }

    #[test]
    fn a_failing_unit_test_fails_the_test_step_with_its_output() {
        let fixture = TaskFixture::new("");

        let report = grade(
            &fixture,
            "#[test]\nfn fails() {\n    panic!(\"boom\");\n}\n",
        );

        assert_eq!(report.steps[0].outcome, Outcome::Passed);
        assert_eq!(report.steps[1].kind, StepKind::TestLib);
        assert!(
            matches!(&report.steps[1].outcome, Outcome::Failed { stdout, .. } if stdout.contains("boom")),
            "{report:?}"
        );
    }

    #[test]
    fn the_fmt_check_runs_only_when_the_task_asks() {
        let unformatted = "pub fn   spaced ( ) {}\n";

        let report = grade(&TaskFixture::new(""), unformatted);
        assert_eq!(report.steps.len(), 3);

        let fixture = TaskFixture::new("fmt_check = true\n");
        let report = grade(&fixture, unformatted);
        assert_eq!(report.steps[3].kind, StepKind::Fmt);
        assert!(
            matches!(report.steps[3].outcome, Outcome::Failed { .. }),
            "{report:?}"
        );

        let report = grade(&fixture, "pub fn formatted() {}\n");
        assert_eq!(report.steps[3].outcome, Outcome::Passed);
    }

    /// Grades `lib_rs` against the hidden check `works` rewritten as `check`.
    fn grade_check(lib_rs: &str, check: &str) -> Grading {
        let fixture = TaskFixture::new("");
        fs::write(fixture.check_file("works"), check).unwrap();
        let submission = fixture.submission();
        fs::write(submission.path().join("src/lib.rs"), lib_rs).unwrap();
        let logs = TempDir::new().unwrap();
        super::grade(
            fixture.evals(),
            fixture.task(),
            submission.path(),
            &Local,
            &GradeLimits::default(),
            logs.path(),
        )
    }

    fn hidden_outcome(grading: &Grading) -> &Outcome {
        let Grading::Graded(report) = grading else {
            panic!("expected a report, got {grading:?}");
        };
        &report.steps[2].outcome
    }

    const DOUBLE_CHECK: &str =
        "#[test]\nfn doubles() {\n    assert_eq!(demo::double(21), 42);\n}\n";

    #[test]
    fn a_hidden_check_passes_against_a_correct_submission() {
        let grading = grade_check("pub fn double(x: i32) -> i32 { x * 2 }", DOUBLE_CHECK);

        assert_eq!(hidden_outcome(&grading), &Outcome::Passed);
    }

    #[test]
    fn a_hidden_check_that_does_not_compile_fails_the_submission() {
        let grading = grade_check("pub fn triple(x: i32) -> i32 { x * 3 }", DOUBLE_CHECK);

        assert!(
            matches!(hidden_outcome(&grading), Outcome::Failed { stderr, .. } if stderr.contains("double")),
            "{grading:?}"
        );
    }

    #[test]
    fn a_failing_hidden_check_fails_the_submission() {
        let grading = grade_check("pub fn double(x: i32) -> i32 { x * 3 }", DOUBLE_CHECK);

        assert!(
            matches!(hidden_outcome(&grading), Outcome::Failed { stdout, .. } if stdout.contains("63")),
            "{grading:?}"
        );
    }

    #[test]
    fn exiting_before_the_summary_is_unconfirmed() {
        let grading = grade_check(
            "pub fn double(_: i32) -> i32 { std::process::exit(0) }",
            DOUBLE_CHECK,
        );

        assert!(
            matches!(hidden_outcome(&grading), Outcome::Unconfirmed { .. }),
            "{grading:?}"
        );
    }

    #[test]
    fn a_partial_fake_summary_is_unconfirmed() {
        let grading = grade_check(
            "pub fn double(_: i32) -> i32 { println!(\"test result: ok. 1 passed\"); std::process::exit(0) }",
            DOUBLE_CHECK,
        );

        assert!(
            matches!(hidden_outcome(&grading), Outcome::Unconfirmed { .. }),
            "{grading:?}"
        );
    }

    #[test]
    fn a_check_listing_two_tests_is_a_task_error() {
        let grading = grade_check(
            "pub fn double(x: i32) -> i32 { x * 2 }",
            "#[test]\nfn first() {}\n#[test]\nfn second() {}\n",
        );

        assert!(
            matches!(
                &grading,
                Grading::Infrastructure(GradeError::Listing {
                    problem: ListingProblem::Count {
                        entries: 2,
                        ignored: 0
                    },
                    ..
                })
            ),
            "{grading:?}"
        );
    }

    #[test]
    fn a_manifest_that_claims_a_hidden_checks_file_is_a_task_error() {
        let fixture = TaskFixture::new("");
        let manifest = format!(
            "{STARTER_MANIFEST}autotests = false\n\n[[test]]\nname = \"smoke\"\npath = \"tests/works.rs\"\n"
        );
        fs::write(fixture.starter().join("Cargo.toml"), manifest).unwrap();
        let submission = fixture.submission();
        let logs = TempDir::new().unwrap();

        let grading = super::grade(
            fixture.evals(),
            fixture.task(),
            submission.path(),
            &Local,
            &GradeLimits::default(),
            logs.path(),
        );

        assert!(
            matches!(&grading, Grading::Infrastructure(GradeError::CheckTarget { found, .. }) if found.is_empty()),
            "{grading:?}"
        );
    }

    #[test]
    fn a_starter_that_declares_a_hidden_checks_target_is_a_task_error() {
        let fixture = TaskFixture::new("");
        let manifest = format!(
            "{STARTER_MANIFEST}autotests = false\n\n[[test]]\nname = \"works\"\npath = \"tests/works.rs\"\nharness = false\n"
        );
        fs::write(fixture.starter().join("Cargo.toml"), manifest).unwrap();
        let submission = fixture.submission();
        let logs = TempDir::new().unwrap();

        let grading = super::grade(
            fixture.evals(),
            fixture.task(),
            submission.path(),
            &Local,
            &GradeLimits::default(),
            logs.path(),
        );

        assert!(
            matches!(&grading, Grading::Infrastructure(GradeError::CheckDeclared { check }) if check.as_str() == "works"),
            "{grading:?}"
        );
    }

    #[test]
    fn a_submission_cannot_replace_a_placed_hidden_check() {
        let fixture = TaskFixture::new("editable = [\"src/**\", \"tests/**\"]\n");
        let submission = fixture.submission();
        fs::write(submission.path().join("src/lib.rs"), "").unwrap();
        fs::create_dir_all(submission.path().join("tests")).unwrap();
        fs::write(
            submission.path().join("tests/works.rs"),
            "#[test]\nfn works() {}\n",
        )
        .unwrap();
        fs::write(fixture.check_file("works"), DOUBLE_CHECK).unwrap();
        let logs = TempDir::new().unwrap();

        let grading = super::grade(
            fixture.evals(),
            fixture.task(),
            submission.path(),
            &Local,
            &GradeLimits::default(),
            logs.path(),
        );

        // The real check calls `double`, which this submission lacks.
        assert!(
            matches!(hidden_outcome(&grading), Outcome::Failed { .. }),
            "{grading:?}"
        );
    }

    #[test]
    fn a_library_test_failure_does_not_skip_the_hidden_checks() {
        let grading = grade_check(
            "pub fn double(x: i32) -> i32 { x * 2 }\n#[test]\nfn fails() { panic!() }\n",
            DOUBLE_CHECK,
        );

        let Grading::Graded(report) = &grading else {
            panic!("{grading:?}");
        };
        assert!(matches!(report.steps[1].outcome, Outcome::Failed { .. }));
        assert_eq!(report.steps[2].outcome, Outcome::Passed);
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
    use crate::task::{CheckName, Task, TaskName};

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

        pub(crate) fn check_file(&self, check: &str) -> PathBuf {
            self.evals
                .check_file(self.task.name(), &CheckName::new(check).unwrap())
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
