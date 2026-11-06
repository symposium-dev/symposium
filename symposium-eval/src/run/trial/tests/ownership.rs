//! Runner-installed files must not become part of the code being graded.

use super::*;
use crate::grade::copy_exact;

#[test]
fn an_installed_skill_is_not_imported_into_grading() {
    assert_clean_grading(&skill_trial_result(&skill_bench("installed")));
}

#[test]
fn a_used_only_skill_is_not_imported_into_grading() {
    assert_clean_grading(&skill_trial_result(&skill_bench("used")));
}

#[test]
fn an_unnamed_sibling_skill_is_not_imported_into_grading() {
    let mut bench = skill_bench("installed");
    add_sibling_skill(&mut bench);

    let result = skill_trial_result(&bench);

    assert!(
        bench
            .trial_root(Configuration::Symposium)
            .join("project/.claude/skills/rust-testing/SKILL.md")
            .is_file(),
        "the session must have installed the unnamed sibling"
    );
    assert_clean_grading(&result);
}

#[test]
fn altered_skill_contents_remain_in_the_grading_submission() {
    assert_modified_skill_is_graded(Session::ChangesSkillContents);
}

#[test]
fn altered_skill_metadata_remains_in_the_grading_submission() {
    assert_modified_skill_is_graded(Session::ChangesSkillMarker);
}

fn assert_modified_skill_is_graded(session: Session) {
    let bench = skill_bench("used");

    let result = bench.result(Configuration::Symposium, &Scripted::new(session));

    assert_eq!(result["code"]["build"], "pass", "{result:#}");
    assert_eq!(
        result["code"]["hidden"]["double"], "fail",
        "an altered skill must remain an agent edit in the grading copy: {result:#}"
    );
}

fn skill_trial_result(bench: &Bench) -> Value {
    let sandbox = Scripted::new(Session::InstallsSkill);

    let result = bench.result(Configuration::Symposium, &sandbox);

    let installed = bench
        .trial_root(Configuration::Symposium)
        .join("project/.claude/skills/rust-formatting/SKILL.md");
    assert!(
        installed.is_file(),
        "the session must have installed the skill"
    );
    assert_eq!(result["session"]["ended"]["status"], "completed");
    assert_eq!(result["code"]["build"], "pass", "{result:#}");
    result
}

fn assert_clean_grading(result: &Value) {
    assert_eq!(
        result["code"]["hidden"]["double"], "pass",
        "a runner-installed skill must not reach the grading copy: {result:#}"
    );
    assert_eq!(
        result["code"]["edits_outside_editable"],
        serde_json::json!([])
    );
}

fn skill_bench(assertion: &str) -> Bench {
    let mut bench = Bench::new();
    let name = bench.inputs.task().name().clone();
    let task = bench.inputs.evals().task_dir(&name);
    fs::write(
        task.join("task.toml"),
        format!(
            "kind = \"regression\"\nplugins = [\"format-on-stop\"]\nprompt = \"Double a number.\"\neditable = [\"src/**\", \".claude/**\"]\n\n[expected.skills]\n{assertion} = [\"rust-formatting\"]\n"
        ),
    )
    .unwrap();
    fs::write(
        task.join("checks/double.rs"),
        r#"#[test]
fn doubles_without_runner_skills() {
    assert_eq!(formatting::double(21), 42);
    assert!(
        !std::path::Path::new(".claude/skills").exists(),
        "runner-installed skills leaked into the grading copy"
    );
}
"#,
    )
    .unwrap();
    bench.inputs = Inputs::reopen(&bench.batch, &name).unwrap();
    bench
}

fn add_sibling_skill(bench: &mut Bench) {
    let skill = bench
        .batch
        .inputs()
        .join("plugins/format-on-stop/skills/rust-testing");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: rust-testing\ndescription: Write focused Rust tests.\n---\nUse assertions.\n",
    )
    .unwrap();
    bench.inputs = Inputs::reopen(&bench.batch, bench.inputs.task().name()).unwrap();
}

/// Plays the SessionStart sync with the exact loose-skill layout this PR supports.
pub(super) fn install_skill(trial: &Trial, session: Session) {
    let skills = trial.registry().join("format-on-stop/skills");
    for source in fs::read_dir(skills).unwrap() {
        let source = source.unwrap();
        install_copy(trial, &source.path(), &source.file_name());
    }
    let copy = trial.project().join(".claude/skills/rust-formatting");
    match session {
        Session::ChangesSkillContents => {
            fs::write(copy.join("SKILL.md"), "changed by the agent").unwrap();
        }
        Session::ChangesSkillMarker => {
            fs::write(copy.join(".symposium"), "changed by the agent").unwrap();
        }
        _ => {}
    }
}

fn install_copy(trial: &Trial, source: &Path, name: &std::ffi::OsStr) {
    let copy = trial.project().join(".claude/skills").join(name);
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    copy_exact(source, &copy, "test skill").unwrap();
    fs::write(copy.join(".symposium"), "").unwrap();
    fs::write(copy.join(".gitignore"), "*\n").unwrap();
}
