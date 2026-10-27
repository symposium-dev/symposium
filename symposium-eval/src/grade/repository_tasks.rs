//! The tasks in `evals/`, graded the ways RFD step 1 asks: the solution passes, and
//! nothing an agent writes outside `editable` changes the grade.

use std::fs;

use tempfile::TempDir;

use super::import::{Change, OutsideEdit};
use super::limits::GradeLimits;
use super::process::Local;
use super::solution::overlay;
use super::{Grading, Outcome, Report, StepKind, grade};
use crate::evals::Evals;
use crate::task::{CheckName, Task, TaskName};

const STORE_CHECK: &str = "settings_survive_reopen";

fn load(name: &str) -> (Evals, Task) {
    let evals = Evals::in_repository();
    let task = Task::load(&evals, &TaskName::new(name).unwrap()).unwrap();
    (evals, task)
}

/// A scratch directory holding `submission/`, the task's solution laid over its starter.
fn solved(evals: &Evals, task: &Task) -> TempDir {
    overlay(evals, task, &evals.solution_dir(task.name())).unwrap()
}

fn grade_scratch(evals: &Evals, task: &Task, scratch: &TempDir) -> Report {
    match grade(
        evals,
        task,
        &scratch.path().join("submission"),
        &Local,
        &GradeLimits::default(),
        &scratch.path().join("logs"),
    ) {
        Grading::Graded(report) => report,
        other => panic!("expected a report, got {other:?}"),
    }
}

fn store_check_outcome(report: &Report) -> &Outcome {
    let kind = StepKind::Hidden(CheckName::new(STORE_CHECK).unwrap());
    &report
        .steps
        .iter()
        .find(|step| step.kind == kind)
        .expect("the hidden check ran")
        .outcome
}

fn assert_all_passed(report: &Report) {
    for step in &report.steps {
        assert_eq!(step.outcome, Outcome::Passed, "{step}");
    }
}

/// The solution's `src/lib.rs` without its final flush.
fn without_flush(scratch: &TempDir) {
    let lib_rs = scratch.path().join("submission/src/lib.rs");
    let source = fs::read_to_string(&lib_rs).unwrap();
    let unflushed = source.replace("    store.flush()\n", "    Ok(())\n");
    assert_ne!(source, unflushed, "the solution ends with a flush");
    fs::write(lib_rs, unflushed).unwrap();
}

#[test]
fn every_task_in_the_repository_passes_check_tasks() {
    crate::check_tasks(&Evals::in_repository()).unwrap();
}

#[test]
fn forgetting_to_flush_fails_the_hidden_check() {
    let (evals, task) = load("store-durability");
    let scratch = solved(&evals, &task);
    without_flush(&scratch);

    let report = grade_scratch(&evals, &task, &scratch);

    assert!(
        matches!(store_check_outcome(&report), Outcome::Failed { .. }),
        "{report:?}"
    );
}

#[test]
fn extra_unit_tests_do_not_change_the_grade() {
    let (evals, task) = load("store-durability");
    let scratch = solved(&evals, &task);
    let lib_rs = scratch.path().join("submission/src/lib.rs");
    let mut source = fs::read_to_string(&lib_rs).unwrap();
    // One shares the hidden check's name, and one is ignored.
    source.push_str(
        "\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn settings_survive_reopen() {}\n\n    #[test]\n    #[ignore]\n    fn ignored() {}\n}\n",
    );
    fs::write(&lib_rs, source).unwrap();

    let report = grade_scratch(&evals, &task, &scratch);

    assert_all_passed(&report);
}

#[test]
fn an_edited_manifest_does_not_change_the_grade() {
    let (evals, task) = load("store-durability");
    let scratch = solved(&evals, &task);
    let manifest = scratch.path().join("submission/Cargo.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[dependencies.missing]\npath = \"../nowhere\"\n");
    fs::write(&manifest, text).unwrap();

    let report = grade_scratch(&evals, &task, &scratch);

    assert_all_passed(&report);
    assert_eq!(
        report.outside_editable,
        [OutsideEdit {
            path: "Cargo.toml".to_owned(),
            change: Change::Modified,
        }]
    );
}

#[test]
fn an_edited_fixture_does_not_change_the_grade() {
    let (evals, task) = load("store-durability");
    let scratch = solved(&evals, &task);
    without_flush(&scratch);
    // Beside the project, as in a trial: this fixture writes through on every `put`.
    let fixture = scratch.path().join("fixtures/fixture-store");
    fs::create_dir_all(fixture.join("src")).unwrap();
    let trusted = evals.fixture_dir(&task.fixtures()[0]);
    fs::copy(trusted.join("Cargo.toml"), fixture.join("Cargo.toml")).unwrap();
    let source = fs::read_to_string(trusted.join("src/lib.rs")).unwrap();
    let writes_through = source.replace(
        "self.entries.insert(key.to_owned(), value.to_owned());\n        Ok(())",
        "self.entries.insert(key.to_owned(), value.to_owned());\n        self.flush()",
    );
    assert_ne!(source, writes_through, "`put` ends by returning `Ok(())`");
    fs::write(fixture.join("src/lib.rs"), writes_through).unwrap();

    let report = grade_scratch(&evals, &task, &scratch);

    assert!(
        matches!(store_check_outcome(&report), Outcome::Failed { .. }),
        "{report:?}"
    );
}
