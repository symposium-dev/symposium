//! Agent tasks: one directory under `evals/tasks/`, described by its `task.toml`.
//!
//! Parsing has two layers, as Cargo does with `Cargo.toml`: `schema` mirrors the file, and
//! [`Task`] is the validated form with defaults applied. Names and patterns are checked
//! while deserializing; the cross-field rules from the RFD reference ("Task format") run
//! afterwards and are all collected, so an author can fix a task in one pass.
//!
//! Task, starter, plugin, fixture, and check directories must be real directories, not
//! symlinks: their contents cross into trials and grading, which must see exactly what
//! the checkout holds. The starter ships a `Cargo.lock`, since grading runs Cargo with
//! `--locked`, and `editable` never reaches its protected build files.

mod checks;
mod error;
mod name;
mod pattern;
mod protected;
mod schema;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub(crate) use checks::CheckError;
pub(crate) use error::{DirProblem, TaskError, TaskErrors};
pub(crate) use name::{
    CheckName, FixtureName, HookName, NameError, PluginName, SkillName, TaskName,
};
pub(crate) use pattern::EditablePattern;

use crate::evals::Evals;
use crate::tree::{self, EntryKind};
use schema::{RawSkills, RawTask};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TaskKind {
    /// Expectations decide success: does Symposium still deliver?
    Regression,
    /// Only code checks decide success: does Symposium improve the outcome?
    Capability,
}

/// A task that passed every rule. Fields are private so the rules cannot be broken after
/// loading.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "trials and grading read tasks in later RFD steps")]
pub(crate) struct Task {
    name: TaskName,
    kind: TaskKind,
    prompt: String,
    expected_output: Option<String>,
    plugins: Vec<PluginName>,
    fixtures: Vec<FixtureName>,
    editable: Vec<EditablePattern>,
    fmt_check: bool,
    checks: Vec<CheckName>,
    expected_hooks: Vec<HookName>,
    expected_skills: SkillExpectations,
}

/// The four skill lists with `used` resolved; no skill is in two lists that contradict
/// each other.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "expectation checks read these in a later RFD step"
)]
pub(crate) struct SkillExpectations {
    installed: Vec<SkillName>,
    not_installed: Vec<SkillName>,
    used: Vec<SkillName>,
    not_used: Vec<SkillName>,
}

/// One of the `[expected.skills]` lists, named as in `task.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillList {
    Installed,
    NotInstalled,
    Used,
    NotUsed,
}

impl Task {
    /// Loads and validates `evals/tasks/<name>/`.
    ///
    /// # Errors
    ///
    /// Reading and parsing `task.toml` stop at the first error; every other rule reports
    /// all of its problems.
    pub(crate) fn load(evals: &Evals, name: &TaskName) -> Result<Self, TaskErrors> {
        let text =
            fs::read_to_string(evals.task_dir(name).join("task.toml")).map_err(TaskError::Read)?;
        let raw: RawTask = toml::from_str(&text).map_err(TaskError::Parse)?;
        Self::validate(evals, name, raw)
    }

    /// Applies the rules a single field cannot express, as Cargo's `to_real_manifest`
    /// turns a parsed `TomlManifest` into a `Manifest`.
    fn validate(evals: &Evals, name: &TaskName, raw: RawTask) -> Result<Self, TaskErrors> {
        let dir = evals.task_dir(name);
        let editable = raw.editable.unwrap_or_else(default_editable);

        let mut errors = Vec::new();
        if raw.prompt.trim().is_empty() {
            errors.push(TaskError::EmptyPrompt);
        }
        errors.extend(Self::directory_errors(
            evals,
            &dir,
            &raw.plugins,
            &raw.fixtures,
            &editable,
        ));
        let checks = checks::load(&dir.join("checks")).unwrap_or_else(|check_errors| {
            errors.extend(check_errors.into_iter().map(TaskError::from));
            Vec::new()
        });
        errors.extend(duplicates("plugins", &raw.plugins));
        errors.extend(duplicates("fixtures", &raw.fixtures));
        errors.extend(duplicates("editable", &editable));
        errors.extend(duplicates("hooks", &raw.expected.hooks));
        let expected_skills = resolve_skills(raw.expected.skills, &mut errors);
        if !errors.is_empty() {
            return Err(TaskErrors(errors));
        }

        Ok(Self {
            name: name.clone(),
            kind: raw.kind,
            prompt: raw.prompt,
            expected_output: raw.expected_output,
            plugins: raw.plugins,
            fixtures: raw.fixtures,
            editable,
            fmt_check: raw.fmt_check,
            checks,
            expected_hooks: raw.expected.hooks,
            expected_skills,
        })
    }

    /// `starter/` with its lockfile and protected files, then each named plugin and
    /// fixture directory.
    fn directory_errors(
        evals: &Evals,
        dir: &Path,
        plugins: &[PluginName],
        fixtures: &[FixtureName],
        editable: &[EditablePattern],
    ) -> Vec<TaskError> {
        let mut errors = Vec::new();
        let starter = dir.join("starter");
        match require_dir(starter.clone(), TaskError::Starter) {
            Some(error) => errors.push(error),
            None => errors.extend(check_starter(&starter, editable)),
        }
        for plugin in distinct(plugins) {
            errors.extend(require_dir(evals.plugin_dir(plugin), |problem| {
                TaskError::Plugin {
                    name: plugin.clone(),
                    problem,
                }
            }));
        }
        for fixture in distinct(fixtures) {
            errors.extend(require_dir(evals.fixture_dir(fixture), |problem| {
                TaskError::Fixture {
                    name: fixture.clone(),
                    problem,
                }
            }));
        }
        errors
    }
}

impl SkillList {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::NotInstalled => "not_installed",
            Self::Used => "used",
            Self::NotUsed => "not_used",
        }
    }
}

impl fmt::Display for SkillList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// The error, if any, for a path that must be a real directory.
fn require_dir(
    path: PathBuf,
    structural: impl FnOnce(DirProblem) -> TaskError,
) -> Option<TaskError> {
    match DirProblem::of(&path) {
        Ok(None) => None,
        Ok(Some(problem)) => Some(structural(problem)),
        Err(source) => Some(TaskError::Inspect { path, source }),
    }
}

/// The starter's `Cargo.lock`, and one error per `editable` pattern that reaches a
/// protected file in it.
fn check_starter(starter: &Path, editable: &[EditablePattern]) -> Vec<TaskError> {
    let entries = match tree::walk(starter) {
        Ok(entries) => entries,
        Err(error) => return vec![TaskError::StarterTree(error)],
    };
    let mut errors = Vec::new();
    let has_lockfile = entries
        .iter()
        .any(|entry| entry.path == "Cargo.lock" && entry.kind == EntryKind::File);
    if !has_lockfile {
        errors.push(TaskError::MissingLockfile);
    }
    for pattern in distinct(editable) {
        let matcher = pattern.matcher();
        let reached = entries
            .iter()
            .find(|entry| protected::is_protected(&entry.path) && matcher.is_match(&entry.path));
        if let Some(entry) = reached {
            errors.push(TaskError::ProtectedEditable {
                pattern: pattern.clone(),
                path: entry.path.clone(),
            });
        }
    }
    errors
}

fn default_editable() -> Vec<EditablePattern> {
    vec![EditablePattern::new("src/**").expect("the default pattern is valid")]
}

/// Each value once, in first-seen order.
fn distinct<T: AsRef<str>>(values: &[T]) -> impl Iterator<Item = &T> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .filter(move |&value| seen.insert(value.as_ref()))
}

/// One error per value that appears more than once, however many times it repeats.
fn duplicates<'a, T: AsRef<str>>(
    list: &'static str,
    values: &'a [T],
) -> impl Iterator<Item = TaskError> + 'a {
    let mut seen = BTreeSet::new();
    let mut reported = BTreeSet::new();
    values
        .iter()
        .map(AsRef::as_ref)
        .filter(move |value| !seen.insert(*value) && reported.insert(*value))
        .map(move |value| TaskError::Duplicate {
            list,
            value: value.to_owned(),
        })
}

fn resolve_skills(raw: RawSkills, errors: &mut Vec<TaskError>) -> SkillExpectations {
    use SkillList::{Installed, NotInstalled, NotUsed};

    errors.extend(duplicates(Installed.key(), &raw.installed));
    errors.extend(duplicates(NotInstalled.key(), &raw.not_installed));
    errors.extend(duplicates(NotUsed.key(), &raw.not_used));
    errors.extend(overlaps(
        (Installed, &raw.installed),
        (NotInstalled, &raw.not_installed),
    ));
    let used = match raw.used {
        Some(used) => {
            errors.extend(used_errors(&used, &raw.not_installed, &raw.not_used));
            used
        }
        None => derived_used(&raw.installed, &raw.not_used),
    };
    SkillExpectations {
        installed: raw.installed,
        not_installed: raw.not_installed,
        used,
        not_used: raw.not_used,
    }
}

/// The rules for a `used` list the task states.
fn used_errors(
    used: &[SkillName],
    not_installed: &[SkillName],
    not_used: &[SkillName],
) -> Vec<TaskError> {
    use SkillList::{NotInstalled, NotUsed, Used};

    duplicates(Used.key(), used)
        .chain(overlaps((Used, used), (NotUsed, not_used)))
        .chain(overlaps((Used, used), (NotInstalled, not_installed)))
        .collect()
}

/// Every installed skill not listed as unused. A derived `used` cannot contradict
/// anything the `installed` check missed.
fn derived_used(installed: &[SkillName], not_used: &[SkillName]) -> Vec<SkillName> {
    let not_used: BTreeSet<&SkillName> = not_used.iter().collect();
    installed
        .iter()
        .filter(|skill| !not_used.contains(skill))
        .cloned()
        .collect()
}

fn overlaps<'a>(
    (first, first_skills): (SkillList, &'a [SkillName]),
    (second, second_skills): (SkillList, &'a [SkillName]),
) -> impl Iterator<Item = TaskError> + 'a {
    let second_skills: BTreeSet<&SkillName> = second_skills.iter().collect();
    first_skills
        .iter()
        .filter(move |skill| second_skills.contains(skill))
        .map(move |skill| TaskError::Contradiction {
            skill: skill.clone(),
            first,
            second,
        })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const ONE_CHECK: &str = "#[test]\nfn works() {}\n";

    /// An `evals/` tree with one valid-looking task, `demo`: a starter with a lockfile
    /// and one check.
    fn evals_with_task(task_toml: &str) -> (TempDir, Evals) {
        let root = TempDir::new().unwrap();
        let task_dir = root.path().join("tasks/demo");
        fs::create_dir_all(task_dir.join("starter")).unwrap();
        fs::write(task_dir.join("starter/Cargo.lock"), "").unwrap();
        fs::create_dir_all(task_dir.join("checks")).unwrap();
        fs::write(task_dir.join("checks/works.rs"), ONE_CHECK).unwrap();
        fs::write(task_dir.join("task.toml"), task_toml).unwrap();
        let evals = Evals::new(root.path());
        (root, evals)
    }

    fn load_demo(evals: &Evals) -> Result<Task, TaskErrors> {
        Task::load(evals, &TaskName::new("demo").unwrap())
    }

    fn skills(names: &[&str]) -> Vec<SkillName> {
        names
            .iter()
            .map(|name| SkillName::new(*name).unwrap())
            .collect()
    }

    fn messages(errors: &TaskErrors) -> Vec<String> {
        errors.as_slice().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn minimal_task_gets_the_documented_defaults() {
        let (_root, evals) =
            evals_with_task("kind = \"regression\"\nprompt = \"Add a function.\"\n");

        let task = load_demo(&evals).unwrap();

        assert_eq!(task.kind, TaskKind::Regression);
        assert_eq!(task.editable, default_editable());
        assert!(!task.fmt_check);
        assert_eq!(task.checks, [CheckName::new("works").unwrap()]);
        assert_eq!(task.expected_skills.used, Vec::<SkillName>::new());
    }

    #[test]
    fn absent_used_means_installed_minus_not_used() {
        let (_root, evals) = evals_with_task(
            r#"
            kind = "regression"
            prompt = "Add a function."
            [expected.skills]
            installed = ["first-skill", "second-skill"]
            not_used = ["second-skill"]
            "#,
        );

        let task = load_demo(&evals).unwrap();

        assert_eq!(task.expected_skills.used, skills(&["first-skill"]));
    }

    #[test]
    fn explicit_empty_used_makes_no_claim() {
        let (_root, evals) = evals_with_task(
            r#"
            kind = "capability"
            prompt = "Add a function."
            [expected.skills]
            installed = ["first-skill"]
            used = []
            "#,
        );

        let task = load_demo(&evals).unwrap();

        assert_eq!(task.expected_skills.used, Vec::<SkillName>::new());
    }

    #[test]
    fn invalid_values_are_parse_errors() {
        for task_toml in [
            "kind = \"regression\"\nprompt = \"x\"\n[expected.skills]\ninstaled = [\"a\"]\n",
            "kind = \"benchmark\"\nprompt = \"x\"\n",
            "kind = \"regression\"\nprompt = \"x\"\nplugins = [\"../escape\"]\n",
            "kind = \"regression\"\nprompt = \"x\"\neditable = [\"src/[abc\"]\n",
            "kind = \"regression\"\nprompt = \"x\"\n[expected]\nhooks = [\"\"]\n",
        ] {
            let (_root, evals) = evals_with_task(task_toml);

            let errors = load_demo(&evals).unwrap_err();

            assert!(
                matches!(errors.as_slice(), [TaskError::Parse(_)]),
                "{task_toml}"
            );
        }
    }

    #[test]
    fn every_contradiction_is_reported() {
        let (_root, evals) = evals_with_task(
            r#"
            kind = "regression"
            prompt = "Add a function."
            [expected.skills]
            installed = ["a"]
            not_installed = ["a", "b"]
            used = ["b", "c"]
            not_used = ["c"]
            "#,
        );

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "skill `a` is in both `installed` and `not_installed`",
                "skill `c` is in both `used` and `not_used`",
                "skill `b` is in both `used` and `not_installed`",
            ]
        );
    }

    #[test]
    fn duplicates_are_reported_once_per_value() {
        let (_root, evals) = evals_with_task(
            r#"
            kind = "regression"
            prompt = "Add a function."
            plugins = ["p", "p", "p"]
            editable = ["src/**", "src/**"]
            [expected]
            hooks = ["on-stop", "on-stop"]
            [expected.skills]
            installed = ["a", "a"]
            "#,
        );

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "plugin `p` in evals/plugins/ does not exist",
                "`plugins` lists `p` more than once",
                "`editable` lists `src/**` more than once",
                "`hooks` lists `on-stop` more than once",
                "`installed` lists `a` more than once",
            ]
        );
    }

    #[test]
    fn directory_problems_are_all_reported() {
        let (root, evals) = evals_with_task(
            "kind = \"capability\"\nprompt = \"  \"\nplugins = [\"present\", \"absent\"]\nfixtures = [\"file\"]\n",
        );
        fs::create_dir_all(root.path().join("plugins/present")).unwrap();
        fs::create_dir_all(root.path().join("fixtures")).unwrap();
        fs::write(root.path().join("fixtures/file"), "").unwrap();
        fs::remove_dir_all(root.path().join("tasks/demo/starter")).unwrap();
        fs::remove_dir_all(root.path().join("tasks/demo/checks")).unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "`prompt` is empty",
                "`starter/` does not exist",
                "plugin `absent` in evals/plugins/ does not exist",
                "fixture `file` in evals/fixtures/ is not a directory",
                "`checks/` does not exist",
            ]
        );
    }

    #[test]
    fn starter_without_a_lockfile_is_rejected() {
        let (root, evals) = evals_with_task("kind = \"regression\"\nprompt = \"x\"\n");
        fs::remove_file(root.path().join("tasks/demo/starter/Cargo.lock")).unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            ["`starter/` has no `Cargo.lock`; grading runs Cargo with `--locked`"]
        );
    }

    #[test]
    fn editable_patterns_cannot_reach_protected_starter_files() {
        let (root, evals) = evals_with_task(
            r#"
            kind = "regression"
            prompt = "x"
            editable = ["src/**", "member/**", ".cargo/*", "build.rs"]
            "#,
        );
        let starter = root.path().join("tasks/demo/starter");
        fs::create_dir_all(starter.join("src")).unwrap();
        fs::create_dir_all(starter.join("member/src")).unwrap();
        fs::create_dir_all(starter.join(".cargo")).unwrap();
        fs::write(starter.join("src/lib.rs"), "").unwrap();
        fs::write(starter.join("member/Cargo.toml"), "").unwrap();
        fs::write(starter.join("member/src/lib.rs"), "").unwrap();
        fs::write(starter.join(".cargo/config.toml"), "").unwrap();
        fs::write(starter.join("build.rs"), "").unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "editable pattern `member/**` matches `starter/member/Cargo.toml`, which grading always takes from the starter",
                "editable pattern `.cargo/*` matches `starter/.cargo/config.toml`, which grading always takes from the starter",
                "editable pattern `build.rs` matches `starter/build.rs`, which grading always takes from the starter",
            ]
        );
    }

    #[test]
    fn every_check_file_is_validated() {
        let (root, evals) = evals_with_task("kind = \"regression\"\nprompt = \"x\"\n");
        let checks = root.path().join("tasks/demo/checks");
        fs::write(
            checks.join("two.rs"),
            "#[test] fn a() {}\n#[test] fn b() {}\n",
        )
        .unwrap();
        fs::write(checks.join("notes.txt"), "").unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "`checks/notes.txt` is not a regular `.rs` file",
                "`checks/two.rs` holds 2 top-level tests; it must hold exactly one",
            ]
        );
    }

    #[test]
    fn empty_checks_directory_is_rejected() {
        let (root, evals) = evals_with_task("kind = \"regression\"\nprompt = \"x\"\n");
        fs::remove_file(root.path().join("tasks/demo/checks/works.rs")).unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(messages(&errors), ["`checks/` holds no hidden tests"]);
    }

    #[test]
    fn task_directory_without_task_toml_is_a_read_error() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("tasks/demo/starter")).unwrap();

        let errors = load_demo(&Evals::new(root.path())).unwrap_err();

        assert!(
            matches!(errors.as_slice(), [TaskError::Read(_)]),
            "{errors}"
        );
    }
}
