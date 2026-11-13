//! Grading: a trusted copy of the task holding only the submission's editable files, then
//! the code checks (RFD reference, "Grading the code").
//!
//! Invariant: only repository-owned code runs outside the `srt` backend. `--check-tasks`
//! grades each task's `solution/` with [`process::Local`]; an agent's submission is
//! graded through `srt`, under the trial's grading policy (`run::grading`).

mod cargo;
mod error;
mod hidden;
mod import;
mod limits;
pub(crate) mod process;
#[cfg(test)]
mod repository_tasks;
mod solution;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) use error::{GradeError, ImportError, ListingProblem, ProcessError, with_causes};
pub(crate) use import::{OutsideEdit, copy_exact, copy_tree};
pub(crate) use solution::check_solution;

use crate::evals::Evals;
use crate::task::{CheckName, Task};
use cargo::{Cargo, StarterTargets};
use import::{GradingCopy, import_submission};
use limits::Budget;
pub(crate) use limits::GradeLimits;
use process::{Backend, Finished, Invocation};

/// What grading concluded about one submission.
#[derive(Debug)]
pub(crate) enum Grading {
    Graded(Report),
    /// The submission cannot be graded, which fails the trial.
    InvalidSubmission(ImportError),
    /// Grading itself failed, which excludes the trial; its submission can be graded
    /// again. `partial` keeps checks completed before the failure, but is never a
    /// grading verdict.
    Infrastructure {
        error: GradeError,
        partial: Option<Report>,
    },
}

#[derive(Debug)]
pub(crate) struct Report {
    /// Sorted by path.
    pub(crate) outside_editable: Vec<OutsideEdit>,
    /// The checks run on the submission, in order.
    pub(crate) steps: Vec<Step>,
}

/// A grading operation failed after zero or more checks completed.
struct Interruption {
    error: Box<GradeError>,
    partial: Option<Report>,
}

impl From<GradeError> for Interruption {
    fn from(error: GradeError) -> Self {
        Self {
            error: Box::new(error),
            partial: None,
        }
    }
}

impl From<ImportError> for Interruption {
    fn from(error: ImportError) -> Self {
        GradeError::from(error).into()
    }
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
    TimedOut {
        /// What went wrong collecting the output once the deadline stopped the command,
        /// with its causes. The timeout still stands.
        after_deadline: Option<String>,
    },
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

impl Grading {
    /// Whether every command grading started was proved stopped and reaped.
    pub(crate) fn containment_is_certain(&self) -> bool {
        match self {
            Self::Graded(report) => report.containment_is_certain(),
            Self::InvalidSubmission(_) => true,
            Self::Infrastructure { error, partial } => {
                grade_error_is_certain(error)
                    && partial.as_ref().is_none_or(Report::containment_is_certain)
            }
        }
    }
}

impl Report {
    fn containment_is_certain(&self) -> bool {
        self.steps.iter().all(|step| {
            !matches!(
                step.outcome,
                Outcome::TimedOut {
                    after_deadline: Some(_)
                }
            )
        })
    }
}

fn grade_error_is_certain(error: &GradeError) -> bool {
    !matches!(
        error,
        GradeError::Process { .. }
            | GradeError::TimedOut {
                after_deadline: Some(_),
                ..
            }
    )
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
            Outcome::TimedOut { after_deadline } => {
                write!(f, "did not finish {kind} in time")?;
                if let Some(problem) = after_deadline {
                    write!(f, ", then {problem}")?;
                }
                return Ok(());
            }
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

/// Where one grading puts its trusted copy, the commands' writable state, and their logs,
/// each a directory a sandbox's policy can name.
#[derive(Debug, Clone)]
pub(crate) struct GradingDirs {
    /// The project and fixtures; must not exist yet.
    pub(crate) sources: PathBuf,
    /// Emptied before each step.
    pub(crate) work: PathBuf,
    pub(crate) logs: PathBuf,
}

impl GradingDirs {
    /// Everything under `dir`, for grading repository-owned code without `srt`.
    pub(crate) fn under(dir: &Path) -> Self {
        Self {
            sources: dir.join("sources"),
            work: dir.join("work"),
            logs: dir.join("logs"),
        }
    }
}

/// Empty directories every trial's project starts with beside the starter, so grading's
/// trusted copy starts with them too. `srt` write-protects `.claude/commands` in the
/// project, and where `.claude/` is missing it protects all of `.claude/` instead, so no
/// skill could be installed.
pub(crate) const PROJECT_SCAFFOLD: [&str; 1] = [".claude"];

/// A submission, with what the runner put in its project that the agent did not, so
/// only the agent's changes are reported as its edits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Submitted<'a> {
    project: &'a Path,
    /// Empty directories, `/`-joined, added to the starter before the agent started.
    scaffold: &'static [&'static str],
    /// Skill copies, `/`-joined and relative to the project, shown to be wholly
    /// Symposium's (`expect::ownership`).
    symposium_copies: &'a [String],
}

impl<'a> Submitted<'a> {
    /// A project the runner added nothing to, such as a task's solution.
    pub(crate) fn bare(project: &'a Path) -> Self {
        Self {
            project,
            scaffold: &[],
            symposium_copies: &[],
        }
    }

    /// A trial's project, which started as the starter and [`PROJECT_SCAFFOLD`], holding
    /// `symposium_copies`.
    pub(crate) fn trial(project: &'a Path, symposium_copies: &'a [String]) -> Self {
        Self {
            project,
            scaffold: &PROJECT_SCAFFOLD,
            symposium_copies,
        }
    }
}

/// Grades `submission` against `task`, running commands through `backend`, with the
/// copy and the commands' logs where `dirs` says.
pub(crate) fn grade(
    evals: &Evals,
    task: &Task,
    submission: Submitted<'_>,
    backend: &impl Backend,
    limits: &GradeLimits,
    dirs: &GradingDirs,
) -> Grading {
    match try_grade(evals, task, submission, backend, limits, dirs) {
        Ok(report) => Grading::Graded(report),
        Err(Interruption { error, partial }) => match (*error, partial) {
            (GradeError::Import(error), None) if error.invalidates_submission() => {
                Grading::InvalidSubmission(error)
            }
            (error, partial) => Grading::Infrastructure { error, partial },
        },
    }
}

/// Checks the trusted task inputs and the grading toolchain without importing or
/// running a submission.
///
/// This is the same preparation [`grade`] performs for every trial: it copies the
/// starter and fixtures, resolves the starter with Cargo, places and verifies every
/// hidden check, and verifies `cargo fmt` when the task requires formatting.
///
/// # Errors
///
/// When trusted inputs cannot be copied, a required command cannot be run or does not
/// succeed, or the starter and hidden checks do not have the expected Cargo targets.
pub(crate) fn preflight_task(
    evals: &Evals,
    task: &Task,
    backend: &impl Backend,
    limits: &GradeLimits,
    dirs: &GradingDirs,
) -> Result<(), GradeError> {
    fs::create_dir_all(&dirs.logs)
        .map_err(|source| ImportError::Write {
            path: dirs.logs.clone(),
            source,
        })
        .map_err(GradeError::from)?;
    let mut commands = Commands {
        backend,
        budget: Budget::new(limits.commands),
        logs: &dirs.logs,
    };
    prepare(evals, task, &PROJECT_SCAFFOLD, dirs, &mut commands, limits)?;
    Ok(())
}

fn try_grade(
    evals: &Evals,
    task: &Task,
    submission: Submitted<'_>,
    backend: &impl Backend,
    limits: &GradeLimits,
    dirs: &GradingDirs,
) -> Result<Report, Interruption> {
    fs::create_dir_all(&dirs.logs).map_err(|source| ImportError::Write {
        path: dirs.logs.clone(),
        source,
    })?;
    let mut commands = Commands {
        backend,
        budget: Budget::new(limits.commands),
        logs: &dirs.logs,
    };
    let prepared = prepare(
        evals,
        task,
        submission.scaffold,
        dirs,
        &mut commands,
        limits,
    )?;
    let outside_editable = import_submission(
        &prepared.copy,
        task.editable(),
        &prepared.starter_owned,
        submission.project,
        submission.symposium_copies,
    )?;
    let steps = match run_steps(task, &prepared, &mut commands, limits) {
        Ok(steps) => steps,
        Err(StepsInterrupted { steps, error }) => {
            let partial = (!steps.is_empty()).then_some(Report {
                outside_editable,
                steps,
            });
            return Err(Interruption {
                error: Box::new(error),
                partial,
            });
        }
    };
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
    scaffold: &[&str],
    dirs: &GradingDirs,
    commands: &mut Commands<'_, impl Backend>,
    limits: &GradeLimits,
) -> Result<Prepared, GradeError> {
    let copy = GradingCopy::from_trusted(evals, task, scaffold, &dirs.sources, &dirs.work)?;
    let cargo = Cargo::new(copy.project(), copy.work());
    let starter = inspect_starter(&cargo, "metadata", &copy, commands, limits.metadata)?;
    place_checks(evals, task, &copy, &starter.tests)?;
    let placed = inspect_starter(&cargo, "metadata-checks", &copy, commands, limits.metadata)?;
    verify_check_targets(task.checks(), &placed.tests)?;
    if task.fmt_check() {
        preflight_command(
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
    prepared: &Prepared,
    commands: &mut Commands<'_, impl Backend>,
    limits: &GradeLimits,
) -> Result<Vec<Step>, StepsInterrupted> {
    let (cargo, copy) = (&prepared.cargo, &prepared.copy);
    let mut steps = Vec::new();
    let run = (|| -> Result<(), GradeError> {
        // Each step starts from an empty `work/`: the submission's code runs in every
        // step, and must leave nothing there for a later one.
        reset_work(copy, &StepKind::Build)?;
        let build = run_step(StepKind::Build, &cargo.build(), commands, limits.build)?;
        let build_passed = build.outcome == Outcome::Passed;
        let mut later = vec![StepKind::TestLib];
        later.extend(task.checks().iter().cloned().map(StepKind::Hidden));
        if task.fmt_check() {
            later.push(StepKind::Fmt);
        }
        steps.push(build);
        for kind in later {
            if build_passed {
                reset_work(copy, &kind)?;
            }
            let step = match kind {
                _ if !build_passed => Step {
                    kind,
                    outcome: Outcome::Skipped,
                },
                StepKind::Hidden(check) => hidden::run(cargo, commands, &check, limits.test)?,
                StepKind::TestLib => run_step(kind, &cargo.test_lib(), commands, limits.test)?,
                StepKind::Fmt => run_step(kind, &cargo.fmt_check(), commands, limits.fmt)?,
                StepKind::Build => unreachable!("the build runs first"),
            };
            steps.push(step);
        }
        Ok(())
    })();
    match run {
        Ok(()) => Ok(steps),
        Err(error) => Err(StepsInterrupted { steps, error }),
    }
}

struct StepsInterrupted {
    steps: Vec<Step>,
    error: GradeError,
}

fn reset_work(copy: &GradingCopy, before: &StepKind) -> Result<(), GradeError> {
    copy.fresh_work().map_err(|source| GradeError::ResetWork {
        before: before.clone(),
        source,
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
        Outcome::TimedOut {
            after_deadline: finished.after_deadline.as_ref().map(with_causes),
        }
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
    let finished = preflight_command(commands, &cargo.metadata(log_name), "cargo metadata", cap)?;
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
fn preflight_command(
    commands: &mut Commands<'_, impl Backend>,
    invocation: &Invocation,
    step: &str,
    cap: Duration,
) -> Result<Finished, GradeError> {
    let mut finished = commands
        .run(invocation, cap)
        .map_err(|source| GradeError::Process {
            step: step.to_owned(),
            source,
        })?;
    if finished.timed_out() {
        return Err(GradeError::TimedOut {
            step: step.to_owned(),
            after_deadline: finished.after_deadline.take(),
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
    use std::io;
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
            Submitted::bare(submission.path()),
            &Local,
            limits,
            &GradingDirs::under(logs.path()),
        ) {
            Grading::Graded(report) => report,
            other => panic!("expected a report, got {other:?}"),
        }
    }

    fn timeout_report(after_deadline: Option<&str>) -> Report {
        Report {
            outside_editable: Vec::new(),
            steps: vec![Step {
                kind: StepKind::Build,
                outcome: Outcome::TimedOut {
                    after_deadline: after_deadline.map(str::to_owned),
                },
            }],
        }
    }

    #[test]
    fn containment_checks_completed_and_partial_timeout_steps() {
        assert!(Grading::Graded(timeout_report(None)).containment_is_certain());
        assert!(
            !Grading::Graded(timeout_report(Some("output stayed open"))).containment_is_certain()
        );

        let later_error = GradeError::Preflight {
            step: "a later operation".to_owned(),
            stderr: "failed".to_owned(),
        };
        let partial = Grading::Infrastructure {
            error: later_error,
            partial: Some(timeout_report(Some("output stayed open"))),
        };
        assert!(!partial.containment_is_certain());
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

    #[cfg(unix)]
    #[test]
    fn a_test_cannot_block_the_hidden_check_by_removing_directory_access() {
        let grading = grade_check(
            r#"
pub fn double(x: i32) -> i32 { x }

#[test]
fn locks_grading_tmp() {
    use std::os::unix::fs::PermissionsExt;

    let locked = std::env::temp_dir().join("locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::set_permissions(locked, std::fs::Permissions::from_mode(0o000)).unwrap();
}
"#,
            DOUBLE_CHECK,
        );

        let Grading::Graded(report) = grading else {
            panic!("expected completed grading");
        };
        assert_eq!(report.steps[1].outcome, Outcome::Passed);
        assert!(
            matches!(report.steps[2].outcome, Outcome::Failed { .. }),
            "the wrong implementation must reach and fail the hidden check: {report:?}"
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
            Submitted::bare(submission.path()),
            &Local,
            &GradeLimits::default(),
            &GradingDirs::under(logs.path()),
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
                Grading::Infrastructure {
                    error: GradeError::Listing {
                        problem: ListingProblem::Count {
                            entries: 2,
                            ignored: 0
                        },
                        ..
                    },
                    ..
                }
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
            Submitted::bare(submission.path()),
            &Local,
            &GradeLimits::default(),
            &GradingDirs::under(logs.path()),
        );

        assert!(
            matches!(&grading, Grading::Infrastructure {
                error: GradeError::CheckTarget { found, .. },
                ..
            } if found.is_empty()),
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
            Submitted::bare(submission.path()),
            &Local,
            &GradeLimits::default(),
            &GradingDirs::under(logs.path()),
        );

        assert!(
            matches!(&grading, Grading::Infrastructure {
                error: GradeError::CheckDeclared { check },
                ..
            } if check.as_str() == "works"),
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
            Submitted::bare(submission.path()),
            &Local,
            &GradeLimits::default(),
            &GradingDirs::under(logs.path()),
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

    struct FailsOn {
        argument: &'static str,
    }

    impl Backend for FailsOn {
        fn run(
            &self,
            invocation: &Invocation,
            deadline: Duration,
            logs: &Path,
        ) -> Result<Finished, ProcessError> {
            if invocation
                .args
                .iter()
                .any(|argument| argument.to_string_lossy() == self.argument)
            {
                Err(ProcessError::Output(io::Error::other("injected failure")))
            } else {
                Local.run(invocation, deadline, logs)
            }
        }
    }

    fn grade_with_failure(argument: &'static str) -> Grading {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        fs::write(
            submission.path().join("src/lib.rs"),
            "pub fn double(x: i32) -> i32 { x * 2 }",
        )
        .unwrap();
        let logs = TempDir::new().unwrap();
        super::grade(
            fixture.evals(),
            fixture.task(),
            Submitted::bare(submission.path()),
            &FailsOn { argument },
            &GradeLimits::default(),
            &GradingDirs::under(logs.path()),
        )
    }

    #[test]
    fn task_preflight_checks_the_required_formatter() {
        let fixture = TaskFixture::new("fmt_check = true\n");
        let root = TempDir::new().unwrap();

        let error = preflight_task(
            fixture.evals(),
            fixture.task(),
            &FailsOn { argument: "fmt" },
            &GradeLimits::default(),
            &GradingDirs::under(root.path()),
        )
        .unwrap_err();

        assert!(
            matches!(
                error,
                GradeError::Process { ref step, .. } if step == "cargo fmt --version"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_infrastructure_failure_keeps_steps_that_already_completed() {
        let grading = grade_with_failure("--test");

        let Grading::Infrastructure {
            error,
            partial: Some(partial),
        } = grading
        else {
            panic!("expected partial infrastructure evidence: {grading:?}");
        };
        assert!(matches!(error, GradeError::Process { .. }));
        assert_eq!(
            partial
                .steps
                .iter()
                .map(|step| (&step.kind, &step.outcome))
                .collect::<Vec<_>>(),
            [
                (&StepKind::Build, &Outcome::Passed),
                (&StepKind::TestLib, &Outcome::Passed)
            ]
        );
    }

    #[test]
    fn an_infrastructure_failure_before_the_first_check_has_no_partial_report() {
        let grading = grade_with_failure("build");

        assert!(
            matches!(grading, Grading::Infrastructure { partial: None, .. }),
            "{grading:?}"
        );
    }

    /// A command the deadline stopped whose output then stayed open.
    struct StuckOutput;

    impl Backend for StuckOutput {
        fn run(
            &self,
            _invocation: &Invocation,
            _deadline: Duration,
            _logs: &Path,
        ) -> Result<Finished, ProcessError> {
            let mut finished = Finished::never_started();
            finished.after_deadline = Some(ProcessError::OutputNotClosed);
            Ok(finished)
        }
    }

    #[test]
    fn a_preflight_timeout_keeps_the_problem_after_its_deadline() {
        let logs = TempDir::new().unwrap();
        let mut commands = Commands {
            backend: &StuckOutput,
            budget: Budget::new(Duration::from_secs(10)),
            logs: logs.path(),
        };
        let invocation = Cargo::new(logs.path().to_owned(), logs.path()).metadata("metadata");

        let error = preflight_command(
            &mut commands,
            &invocation,
            "cargo metadata",
            Duration::from_secs(1),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            GradeError::TimedOut {
                after_deadline: Some(ProcessError::OutputNotClosed),
                ..
            }
        ));
    }

    #[test]
    fn a_problem_after_the_deadline_reaches_the_report() {
        let mut finished = Finished::never_started();
        finished.after_deadline = Some(ProcessError::Output(std::io::Error::other("disk full")));

        let step = Step {
            kind: StepKind::Build,
            outcome: outcome_of(&finished),
        };

        assert_eq!(
            step.outcome,
            Outcome::TimedOut {
                after_deadline: Some("cannot collect the command's output: disk full".to_owned())
            }
        );
        assert_eq!(
            step.to_string(),
            "did not finish `cargo build` in time, then cannot collect the command's output: disk full"
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

        assert_eq!(
            report.steps[0].outcome,
            Outcome::TimedOut {
                after_deadline: None
            }
        );
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::import::{GradingCopy, OutsideEdit, copy_tree, import_submission};
    use super::{ImportError, Submitted};
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
        _dir: TempDir,
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
            self.import(Submitted::bare(submission), build_scripts)
        }

        /// As [`Self::build_copy`], for a submission the runner prepared.
        pub(crate) fn build_trial_copy(
            &self,
            submission: Submitted<'_>,
        ) -> Result<Imported, ImportError> {
            self.import(submission, &BTreeSet::new())
        }

        fn import(
            &self,
            submission: Submitted<'_>,
            build_scripts: &BTreeSet<String>,
        ) -> Result<Imported, ImportError> {
            let dir = TempDir::new().unwrap();
            let (sources, work) = (dir.path().join("sources"), dir.path().join("work"));
            let copy = GradingCopy::from_trusted(
                &self.evals,
                &self.task,
                submission.scaffold,
                &sources,
                &work,
            )?;
            let edits = import_submission(
                &copy,
                self.task.editable(),
                build_scripts,
                submission.project,
                submission.symposium_copies,
            )?;
            Ok(Imported {
                _dir: dir,
                copy,
                edits,
            })
        }
    }

    impl Imported {
        pub(crate) fn outside_editable(&self) -> &[OutsideEdit] {
            &self.edits
        }
    }
}
