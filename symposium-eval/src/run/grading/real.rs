//! Grading under real `srt`: run on Linux with `cargo test -p symposium-eval -- --ignored`.
//!
//! Each escape is a submission whose own unit test passes only if the escape fails. It
//! is graded twice: under `srt`, where its test must pass, and without, where it must
//! fail, so the escape is shown possible on the host and the sandbox is what stops it.

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use tempfile::TempDir;

use crate::grade::process::Local;
use crate::grade::{GradeLimits, Grading, GradingDirs, Outcome, StepKind, grade};
use crate::run::test_support::{RealSrt, api_key, inputs};
use crate::run::{Configuration, Trial};
use crate::task::TaskName;

fn formatting_trial(real: &RealSrt) -> Trial {
    let name = TaskName::new("formatting").unwrap();
    real.trial_of(&name, 1, Configuration::Baseline)
}

fn test_lib_passed(grading: &Grading) -> bool {
    let Grading::Graded(report) = grading else {
        panic!("expected a report, got {grading:?}");
    };
    report
        .steps
        .iter()
        .any(|step| step.kind == StepKind::TestLib && step.outcome == Outcome::Passed)
}

/// Whether the hidden check `check` ran and passed.
fn step_passed(grading: &Grading, check: &str) -> bool {
    let Grading::Graded(report) = grading else {
        panic!("expected a report, got {grading:?}");
    };
    let kind = StepKind::Hidden(crate::task::CheckName::new(check).unwrap());
    report
        .steps
        .iter()
        .any(|step| step.kind == kind && step.outcome == Outcome::Passed)
}

/// Grades `trial`'s project without a sandbox, as only the repository's own code is.
fn grade_unsandboxed(real: &RealSrt, trial: &Trial) -> Grading {
    let logs = TempDir::new().unwrap();
    let inputs = inputs(real.batch(), "formatting");
    grade(
        inputs.evals(),
        inputs.task(),
        &trial.project(),
        &Local,
        &GradeLimits::default(),
        &GradingDirs::under(logs.path()),
    )
}

/// A `formatting` submission whose one unit test asserts `escape_fails`, an
/// expression true only when the escape it tries failed.
fn submit(trial: &Trial, escape_fails: &str) {
    let lib = format!(
        "pub fn double(x: i32) -> i32 {{\n    x * 2\n}}\n\n#[cfg(test)]\nmod escape {{\n    #[test]\n    fn is_blocked() {{\n        assert!({escape_fails});\n    }}\n}}\n"
    );
    fs::write(trial.project().join("src/lib.rs"), lib).unwrap();
}

/// Grades the submission with srt, where the escape must fail, and without, where it
/// must succeed.
fn assert_only_srt_blocks(real: &RealSrt, trial: &Trial) {
    let sandboxed = real.grade(trial, "formatting", &trial.project());
    assert!(test_lib_passed(&sandboxed), "under srt: {sandboxed:#?}");
    let unsandboxed = grade_unsandboxed(real, trial);
    assert!(
        !test_lib_passed(&unsandboxed),
        "without srt: {unsandboxed:#?}"
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn the_formatting_solution_grades_under_srt() {
    let real = RealSrt::new(api_key());
    let trial = formatting_trial(&real);
    let solution =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/tasks/formatting/solution/src/lib.rs");
    fs::copy(solution, trial.project().join("src/lib.rs")).unwrap();

    let grading = real.grade(&trial, "formatting", &trial.project());

    let Grading::Graded(report) = &grading else {
        panic!("expected a report, got {grading:?}");
    };
    for step in &report.steps {
        assert_eq!(step.outcome, Outcome::Passed, "{}", step.kind);
    }
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_submission_cannot_rewrite_a_hidden_check() {
    let real = RealSrt::new(api_key());
    let trial = formatting_trial(&real);
    let check = trial.grading_sources().join("project/tests/double.rs");
    let rewrite = format!("std::fs::write({check:?}, \"#[test] fn passes() {{}}\").is_err()");
    // A wrong `double`, which the real hidden check fails and a rewritten one would pass.
    let lib = format!(
        "pub fn double(x: i32) -> i32 {{\n    x\n}}\n\n#[cfg(test)]\nmod escape {{\n    #[test]\n    fn is_blocked() {{\n        assert!({rewrite});\n    }}\n}}\n"
    );
    fs::write(trial.project().join("src/lib.rs"), lib).unwrap();

    let grading = real.grade(&trial, "formatting", &trial.project());

    assert!(
        test_lib_passed(&grading),
        "the rewrite was not blocked: {grading:#?}"
    );
    assert!(
        !step_passed(&grading, "double"),
        "a rewritten check ran: {grading:#?}"
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn what_a_step_plants_in_work_never_reaches_the_next() {
    let real = RealSrt::new(api_key());
    let trial = formatting_trial(&real);
    let planted = trial.grading_work().join("cargo-home/config.toml");
    // Were it kept, the hidden check could not even build.
    let plant = format!(
        "{{ std::fs::create_dir_all({:?}).is_ok() && std::fs::write({planted:?}, \"[build]\\nrustc-wrapper = \\\"/bin/false\\\"\\n\").is_ok() }}",
        planted.parent().unwrap()
    );
    submit(&trial, &plant);

    let grading = real.grade(&trial, "formatting", &trial.project());

    assert!(
        test_lib_passed(&grading),
        "the plant was not written: {grading:#?}"
    );
    assert!(
        step_passed(&grading, "double"),
        "the plant reached the next step: {grading:#?}"
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_submission_cannot_read_a_home() {
    let real = RealSrt::new(api_key());
    let trial = formatting_trial(&real);
    let secret = real.decoy_home().join(".bashrc");
    submit(&trial, &format!("std::fs::read({secret:?}).is_err()"));

    assert_only_srt_blocks(&real, &trial);
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_submission_cannot_write_into_the_trial_or_its_logs() {
    for target in ["project/escaped", "grading/logs/escaped"] {
        let real = RealSrt::new(api_key());
        let trial = formatting_trial(&real);
        let path = trial.root().join(target);
        submit(&trial, &format!("std::fs::write({path:?}, \"x\").is_err()"));

        assert_only_srt_blocks(&real, &trial);
    }
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_submission_cannot_reach_the_network() {
    let real = RealSrt::new(api_key());
    let trial = formatting_trial(&real);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // The one connection the unsandboxed control makes; the sandboxed one never arrives.
    let accepting = thread::spawn(move || listener.accept().is_ok());
    submit(
        &trial,
        &format!(
            "std::net::TcpStream::connect({:?}).is_err()",
            address.to_string()
        ),
    );

    assert_only_srt_blocks(&real, &trial);
    assert!(accepting.join().unwrap(), "the control did not connect");
}
