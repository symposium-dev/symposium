//! No work-directory cleanup or command may follow uncertain process containment.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

use super::process::{Backend, Finished, Invocation, Local};
use super::testing::TaskFixture;
use super::{GradeLimits, Grading, GradingDirs, Outcome, ProcessError, Submitted, grade};

/// Runs trusted fixture code, except for one command whose timeout is injected. The
/// marker stands for state a surviving process may still be using.
struct Timeout {
    at: &'static str,
    uncertain: bool,
    marker: PathBuf,
    reached: Cell<bool>,
    later_commands: Cell<usize>,
}

impl Backend for Timeout {
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError> {
        if self.reached.get() {
            self.later_commands.set(self.later_commands.get() + 1);
        }
        if invocation.log_name == self.at {
            self.reached.set(true);
            fs::write(&self.marker, "still in use").unwrap();
            let mut finished = Finished::never_started();
            if self.uncertain {
                finished.after_deadline = Some(ProcessError::OutputNotClosed);
            }
            return Ok(finished);
        }
        Local.run(invocation, deadline, logs)
    }
}

fn timed_out(at: &'static str, uncertain: bool) -> (TempDir, Timeout, Grading) {
    let fixture = TaskFixture::new("fmt_check = true\n");
    let submission = fixture.submission();
    fs::write(submission.path().join("src/lib.rs"), "pub fn works() {}\n").unwrap();
    let root = TempDir::new().unwrap();
    let dirs = GradingDirs::under(root.path());
    let backend = Timeout {
        at,
        uncertain,
        marker: dirs.work.join("still-in-use"),
        reached: Cell::new(false),
        later_commands: Cell::new(0),
    };
    let grading = grade(
        fixture.evals(),
        fixture.task(),
        Submitted::bare(submission.path()),
        &backend,
        &GradeLimits::default(),
        &dirs,
    );
    assert!(backend.reached.get(), "{grading:?}");
    (root, backend, grading)
}

#[test]
fn uncertain_timeouts_stop_before_cleanup_or_another_command() {
    for at in [
        "build",
        "test-lib",
        "hidden-works-list",
        "hidden-works-ignored",
        "hidden-works",
    ] {
        let (_root, backend, grading) = timed_out(at, true);
        let Grading::Graded(report) = grading else {
            panic!("the timeout must remain a submission outcome: {grading:?}");
        };
        assert_eq!(backend.later_commands.get(), 0, "after {at}");
        assert_eq!(
            fs::read_to_string(&backend.marker).unwrap(),
            "still in use",
            "after {at}"
        );
        assert!(
            matches!(
                report.steps.last().unwrap().outcome,
                Outcome::TimedOut {
                    after_deadline: Some(_)
                }
            ),
            "{report:?}"
        );
        assert!(
            report
                .steps
                .iter()
                .any(|step| step.kind == super::StepKind::Build)
        );
    }
}

#[test]
fn a_contained_timeout_still_allows_later_checks() {
    let (_root, backend, grading) = timed_out("test-lib", false);
    assert!(matches!(grading, Grading::Graded(_)), "{grading:?}");
    assert!(backend.later_commands.get() > 0);
    assert!(!backend.marker.exists(), "the next step should reset work");
}
