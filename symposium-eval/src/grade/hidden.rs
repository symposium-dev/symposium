//! Running one hidden check as exactly one test (RFD reference, "Grading the code").
//!
//! Stable libtest has no machine-readable results, so a pass needs three signals to
//! agree: the listing (`--list --format terse`, plain and with `--ignored`) shows exactly
//! one test and none ignored; the run exits successfully; and its last summary line
//! reports exactly one passed test and nothing else. Code under test can print anything,
//! so this is defensive validation that catches a crashed, skipped, or crudely faked run,
//! not an authentication boundary: an exact forged summary followed by a successful exit
//! is indistinguishable from a real one. Isolation is the `srt` backend's job.
//!
//! The listing counts are the task's responsibility: `--check-tasks` already proved one
//! plain test per file from the source, so any other count is a task or runner error.
//! A check that does not compile against the submission is the submission's failure.

use std::time::Duration;

use super::process::Finished;
use super::{
    Commands, GradeError, ListingProblem, Outcome, ProcessError, Step, StepKind, outcome_of,
};
use crate::grade::cargo::Cargo;
use crate::grade::process::Backend;
use crate::task::CheckName;

/// The summary a run of exactly one passing test ends with, up to its timing.
const ONE_PASSED: &str =
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";

/// Runs `check`, already placed in the grading copy as `tests/<check>.rs`.
///
/// # Errors
///
/// When a command cannot be run, or the listing shows something other than one test:
/// both are failures of the task or the machine, not of the submission.
pub(super) fn run(
    cargo: &Cargo,
    commands: &mut Commands<'_, impl Backend>,
    check: &CheckName,
    cap: Duration,
) -> Result<Step, GradeError> {
    let kind = StepKind::Hidden(check.clone());
    let outcome = match check_listing(cargo, commands, check, cap)? {
        Some(failed) => failed,
        None => run_confirmed(cargo, commands, check, cap)?,
    };
    Ok(Step { kind, outcome })
}

/// Lists `check` plainly and with `--ignored`, requiring one test and none ignored. The
/// first listing compiles the check against the submission, so a listing that fails is
/// the submission's outcome, returned as `Some`.
fn check_listing(
    cargo: &Cargo,
    commands: &mut Commands<'_, impl Backend>,
    check: &CheckName,
    cap: Duration,
) -> Result<Option<Outcome>, GradeError> {
    let mut counts = [0; 2];
    for (count, ignored) in counts.iter_mut().zip([false, true]) {
        let listed = commands
            .run(&cargo.list_test(check, ignored), cap)
            .map_err(process_error(check))?;
        if !listed.succeeded() {
            return Ok(Some(outcome_of(&listed)));
        }
        *count = listing(&listed, check)?;
    }
    let [entries, ignored] = counts;
    if entries != 1 || ignored != 0 {
        return Err(GradeError::Listing {
            check: check.clone(),
            problem: ListingProblem::Count { entries, ignored },
        });
    }
    Ok(None)
}

/// Runs `check`, reporting a successful exit without the expected summary as unconfirmed.
fn run_confirmed(
    cargo: &Cargo,
    commands: &mut Commands<'_, impl Backend>,
    check: &CheckName,
    cap: Duration,
) -> Result<Outcome, GradeError> {
    let finished = commands
        .run(&cargo.run_test(check), cap)
        .map_err(process_error(check))?;
    Ok(match outcome_of(&finished) {
        Outcome::Passed if !confirms_one_pass(&finished.stdout.tail()) => Outcome::Unconfirmed {
            stdout: finished.stdout.tail(),
            stderr: finished.stderr.tail(),
            truncated: finished.stdout.truncated() || finished.stderr.truncated(),
        },
        outcome => outcome,
    })
}

/// The error for a command of `check` that could not be run.
fn process_error(check: &CheckName) -> impl Fn(ProcessError) -> GradeError {
    let step = StepKind::Hidden(check.clone()).command();
    move |source| GradeError::Process {
        step: step.clone(),
        source,
    }
}

/// The number of entries in a terse listing: one `name: test` or `name: benchmark` line
/// each.
fn listing(finished: &Finished, check: &CheckName) -> Result<usize, GradeError> {
    let unrecognized = |line: &str| GradeError::Listing {
        check: check.clone(),
        problem: ListingProblem::Unrecognized {
            line: line.to_owned(),
        },
    };
    if finished.stdout.truncated() {
        return Err(unrecognized("(a listing too long to keep)"));
    }
    count_entries(&finished.stdout.tail()).map_err(unrecognized)
}

/// Counts the entries of a terse listing, or returns the first line that is not one.
fn count_entries(listing: &str) -> Result<usize, &str> {
    let mut entries = 0;
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        if line.ends_with(": test") || line.ends_with(": benchmark") {
            entries += 1;
        } else {
            return Err(line);
        }
    }
    Ok(entries)
}

/// Whether the last summary in `stdout` reports exactly one passed test and nothing else.
fn confirms_one_pass(stdout: &str) -> bool {
    stdout
        .lines()
        .rev()
        .find(|line| line.starts_with("test result: "))
        .is_some_and(|summary| {
            summary
                .split_once("; finished in ")
                .map_or(summary, |(counts, _)| counts)
                == ONE_PASSED
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terse_listing_counts_tests_and_benchmarks() {
        assert_eq!(count_entries(""), Ok(0));
        assert_eq!(count_entries("works: test\n"), Ok(1));
        assert_eq!(count_entries("a: test\nb: benchmark\n\n"), Ok(2));
        assert_eq!(
            count_entries("works: test\nrunning 1 test\n"),
            Err("running 1 test")
        );
    }

    #[test]
    fn only_an_exact_final_summary_confirms_one_pass() {
        let real = "running 1 test\ntest works ... ok\n\n\
            test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
            finished in 0.00s\n";
        assert!(confirms_one_pass(real));

        for unconfirmed in [
            "",
            "running 1 test\n",
            "test result: ok. 1 passed\n",
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
             test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        ] {
            assert!(!confirms_one_pass(unconfirmed), "{unconfirmed:?}");
        }
    }
}
