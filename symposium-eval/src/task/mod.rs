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
mod hooks;
mod manifest;
mod name;
mod pattern;
mod protected;
mod schema;
mod skills;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub(crate) use checks::CheckError;
pub(crate) use error::{DirProblem, TaskError, TaskErrors};
pub(crate) use hooks::ExpectedHook;
pub(crate) use name::{
    BatchName, CheckName, FixtureName, HookName, NameError, PluginName, SkillName, TaskName,
    ToolName,
};
pub(crate) use pattern::EditablePattern;
pub(crate) use protected::is_protected;
pub(crate) use skills::SkillSource;

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
    expected_hooks: Vec<ExpectedHook>,
    expected_skills: SkillExpectations,
    skill_sources: Vec<SkillSource>,
}

/// The four skill lists with `used` resolved; no skill is in two lists that contradict
/// each other.
#[derive(Debug, Clone)]
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

impl SkillExpectations {
    /// Skills that must be installed during the session.
    pub(crate) fn installed(&self) -> &[SkillName] {
        &self.installed
    }

    /// Skills that must not be installed.
    pub(crate) fn not_installed(&self) -> &[SkillName] {
        &self.not_installed
    }

    /// Skills the agent must successfully load.
    pub(crate) fn used(&self) -> &[SkillName] {
        &self.used
    }

    /// Skills the agent must not load.
    pub(crate) fn not_used(&self) -> &[SkillName] {
        &self.not_used
    }

    /// Every skill the lists name, once each, in list order.
    pub(crate) fn names(&self) -> impl Iterator<Item = &SkillName> {
        let lists = [
            &self.installed,
            &self.not_installed,
            &self.used,
            &self.not_used,
        ];
        let mut seen = BTreeSet::new();
        lists
            .into_iter()
            .flatten()
            .filter(move |skill| seen.insert(*skill))
    }

    /// Expectations that only assert use.
    #[cfg(test)]
    pub(crate) fn using(used: Vec<SkillName>, not_used: Vec<SkillName>) -> Self {
        Self {
            installed: Vec::new(),
            not_installed: Vec::new(),
            used,
            not_used,
        }
    }

    /// Expectations that only assert installation.
    #[cfg(test)]
    pub(crate) fn installing(installed: Vec<SkillName>, not_installed: Vec<SkillName>) -> Self {
        Self {
            installed,
            not_installed,
            used: Vec::new(),
            not_used: Vec::new(),
        }
    }
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

    pub(crate) fn name(&self) -> &TaskName {
        &self.name
    }

    pub(crate) fn kind(&self) -> TaskKind {
        self.kind
    }

    /// What the agent is asked to do.
    pub(crate) fn prompt(&self) -> &str {
        &self.prompt
    }

    pub(crate) fn fixtures(&self) -> &[FixtureName] {
        &self.fixtures
    }

    pub(crate) fn plugins(&self) -> &[PluginName] {
        &self.plugins
    }

    /// The hidden checks, in name order.
    pub(crate) fn checks(&self) -> &[CheckName] {
        &self.checks
    }

    pub(crate) fn fmt_check(&self) -> bool {
        self.fmt_check
    }

    pub(crate) fn editable(&self) -> &[EditablePattern] {
        &self.editable
    }

    pub(crate) fn expected_skills(&self) -> &SkillExpectations {
        &self.expected_skills
    }

    /// The directory in `evals/` that provides `skill`, when one does.
    pub(crate) fn skill_source(&self, skill: &SkillName) -> Option<&Path> {
        self.skill_sources
            .iter()
            .find(|source| source.name == *skill)
            .map(|source| source.dir.as_path())
    }

    /// The plugin hooks that must leave a successful record, each with its event.
    pub(crate) fn expected_hooks(&self) -> &[ExpectedHook] {
        &self.expected_hooks
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
        errors.extend(check_collisions(&evals.starter_dir(name), &checks));
        errors.extend(duplicates("plugins", &raw.plugins));
        errors.extend(duplicates("fixtures", &raw.fixtures));
        errors.extend(duplicates("editable", &editable));
        errors.extend(duplicates("hooks", &raw.expected.hooks));
        let expected_skills = resolve_skills(raw.expected.skills, &mut errors);
        let (expected_hooks, skill_sources) = match manifest::read(evals, &raw.plugins, &mut errors)
        {
            Some(plugins) => (
                hooks::resolve(&plugins, &raw.expected.hooks, &mut errors),
                skills::resolve(&plugins, &expected_skills, &mut errors),
            ),
            None => (Vec::new(), Vec::new()),
        };
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
            expected_hooks,
            expected_skills,
            skill_sources,
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

/// Grading places each hidden check at `tests/<check>.rs`, so the starter cannot have a
/// file there: it would be silently replaced.
fn check_collisions<'a>(
    starter: &'a Path,
    checks: &'a [CheckName],
) -> impl Iterator<Item = TaskError> + 'a {
    checks.iter().filter_map(move |check| {
        let path = starter.join("tests").join(format!("{check}.rs"));
        match fs::symlink_metadata(&path) {
            Ok(_) => Some(TaskError::CheckCollision(check.clone())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => Some(TaskError::Inspect { path, source }),
        }
    })
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
    use crate::client::HookEvent;

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
            plugins = ["provider"]
            [expected.skills]
            installed = ["first-skill", "second-skill"]
            not_used = ["second-skill"]
            "#,
        );
        provide(&evals, &["first-skill", "second-skill"]);

        let task = load_demo(&evals).unwrap();

        assert_eq!(task.expected_skills.used, skills(&["first-skill"]));
    }

    #[test]
    fn explicit_empty_used_makes_no_claim() {
        let (_root, evals) = evals_with_task(
            r#"
            kind = "capability"
            prompt = "Add a function."
            plugins = ["provider"]
            [expected.skills]
            installed = ["first-skill"]
            used = []
            "#,
        );
        provide(&evals, &["first-skill"]);

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
                "expected skill `a` is not provided by any of the task's plugins",
                "expected skill `b` is not provided by any of the task's plugins",
                "expected skill `c` is not provided by any of the task's plugins",
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
                // Plugin `p` is missing, so nothing is concluded about what it declares.
                "`installed` lists `a` more than once",
            ]
        );
    }

    fn skill_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Use it.\n---\n")
    }

    const SKILLS: &str = "[[skills]]\nsource.path = \"skills\"\n";

    /// Writes `files`, each a `/`-joined path under `evals/plugins/` and its contents.
    fn write_plugins<P: AsRef<str>>(evals: &Evals, files: &[(P, String)]) {
        let any = evals.plugin_dir(&PluginName::new("any").unwrap());
        let plugins = any.parent().unwrap();
        for (path, contents) in files {
            let path = plugins.join(path.as_ref());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    /// Plugin `provider`, whose `skills/` holds a skill for each of `names`.
    fn provide(evals: &Evals, names: &[&str]) {
        let mut files = vec![("provider/Symposium.toml".to_owned(), SKILLS.to_owned())];
        for name in names {
            files.push((format!("provider/skills/{name}/SKILL.md"), skill_md(name)));
        }
        write_plugins(evals, &files);
    }

    /// The `demo` task with `plugins` and the `[expected.skills]` body `skills`, after
    /// writing `files` under `evals/plugins/`.
    fn expecting_skills(
        plugins: &[&str],
        skills: &str,
        files: &[(&str, String)],
    ) -> (TempDir, Result<Task, TaskErrors>) {
        let names: Vec<_> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
        let (root, evals) = evals_with_task(&format!(
            "kind = \"regression\"\nprompt = \"x\"\nplugins = [{}]\n[expected.skills]\n{skills}\n",
            names.join(", ")
        ));
        write_plugins(&evals, files);
        let task = load_demo(&evals);
        (root, task)
    }

    fn skill_name(name: &str) -> SkillName {
        SkillName::new(name).unwrap()
    }

    /// `relative`, written with `/`, under `root`.
    fn under(root: &TempDir, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(root.path().to_owned(), |path, part| path.join(part))
    }

    #[test]
    fn an_expected_skill_is_provided_by_the_skill_md_its_name_is_in() {
        let files = [
            ("first/Symposium.toml", SKILLS.to_owned()),
            // The frontmatter names the skill, not its directory.
            ("first/skills/folder/SKILL.md", skill_md("named-skill")),
        ];
        let (root, task) = expecting_skills(&["first"], r#"installed = ["named-skill"]"#, &files);

        let source = task
            .unwrap()
            .skill_source(&skill_name("named-skill"))
            .map(Path::to_owned);

        assert_eq!(source, Some(under(&root, "plugins/first/skills/folder")));
    }

    #[test]
    fn a_plugin_without_a_manifest_provides_the_skill_at_its_root() {
        let files = [("bare/SKILL.md", skill_md("bare-skill"))];
        let (root, task) = expecting_skills(&["bare"], r#"installed = ["bare-skill"]"#, &files);

        let task = task.unwrap();

        assert_eq!(
            task.skill_source(&skill_name("bare-skill")),
            Some(under(&root, "plugins/bare").as_path())
        );
    }

    #[test]
    fn a_skill_inside_another_or_inside_an_installed_copy_provides_nothing() {
        let files = [
            ("first/Symposium.toml", SKILLS.to_owned()),
            ("first/skills/outer/SKILL.md", skill_md("outer")),
            ("first/skills/outer/inner/SKILL.md", skill_md("inner")),
            ("first/skills/copy/.symposium", String::new()),
            ("first/skills/copy/SKILL.md", skill_md("copied")),
        ];
        let (_root, task) = expecting_skills(
            &["first"],
            r#"installed = ["outer", "inner", "copied"]"#,
            &files,
        );

        assert_eq!(
            messages(&task.unwrap_err()),
            [
                "expected skill `inner` is not provided by any of the task's plugins",
                "expected skill `copied` is not provided by any of the task's plugins",
            ]
        );
    }

    #[test]
    fn only_a_skill_that_must_reach_the_agent_needs_a_provider() {
        let (_root, task) = expecting_skills(
            &[],
            r#"installed = []
            not_installed = ["absent"]
            used = ["unused"]
            not_used = ["absent"]"#,
            &[],
        );

        assert_eq!(
            messages(&task.unwrap_err()),
            ["expected skill `unused` is not provided by any of the task's plugins"]
        );
    }

    #[test]
    fn a_skill_two_sources_provide_is_ambiguous_wherever_it_is_listed() {
        let files = [
            ("first/Symposium.toml", SKILLS.to_owned()),
            ("first/skills/twice/SKILL.md", skill_md("twice")),
            ("second/SKILL.md", skill_md("twice")),
        ];
        for list in ["installed", "not_installed"] {
            let (root, task) = expecting_skills(
                &["first", "second"],
                &format!(r#"{list} = ["twice"]"#),
                &files,
            );

            assert_eq!(
                messages(&task.unwrap_err()),
                [format!(
                    "expected skill `twice` is provided by both `{}` and `{}`",
                    under(&root, "plugins/first/skills/twice").display(),
                    under(&root, "plugins/second").display()
                )],
                "{list}"
            );
        }
    }

    #[test]
    fn a_skill_source_must_be_a_path_inside_the_plugin() {
        let manifest = concat!(
            "[[skills]]\nsource.git = \"https://example.com/skills\"\n",
            "[[skills]]\nsource.path = \"../elsewhere\"\n",
            "[[skills]]\nsource.path = \"missing\"\n",
        );
        let files = [("first/Symposium.toml", manifest.to_owned())];
        let (_root, task) = expecting_skills(&["first"], "installed = []", &files);

        assert_eq!(
            messages(&task.unwrap_err()),
            [
                "a `[[skills]]` group in plugin `first` needs a `source.path`, the only source an offline trial can use",
                "skill source `../elsewhere` in plugin `first` leaves the plugin, which is all a trial copies",
                "skill source `missing` in plugin `first` is not a directory in it",
            ]
        );
    }

    #[test]
    fn a_skill_symposium_would_not_load_is_an_error() {
        let files = [
            ("first/Symposium.toml", SKILLS.to_owned()),
            // Named, so it would match, but Symposium needs a description too.
            (
                "first/skills/undescribed/SKILL.md",
                "---\nname: undescribed\n---\n".to_owned(),
            ),
        ];
        let (root, task) = expecting_skills(&["first"], r#"installed = ["undescribed"]"#, &files);
        let path = under(&root, "plugins/first/skills/undescribed/SKILL.md");

        assert_eq!(
            messages(&task.unwrap_err()),
            [
                format!(
                    "`{}` in plugin `first` would not load: its frontmatter has no `description`",
                    path.display()
                ),
                "expected skill `undescribed` is not provided by any of the task's plugins"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn one_skill_md_reached_through_two_groups_is_one_provider() {
        let manifest = format!(
            "{SKILLS}[[skills]]\nsource.path = \"./skills\"\n[[skills]]\nsource.path = \".\"\n"
        );
        let files = [
            ("first/Symposium.toml", manifest),
            ("first/skills/shared/SKILL.md", skill_md("shared")),
        ];
        let (root, task) = expecting_skills(&["first"], r#"installed = ["shared"]"#, &files);

        assert_eq!(
            task.unwrap().skill_source(&skill_name("shared")),
            Some(under(&root, "plugins/first/skills/shared").as_path())
        );
    }

    #[test]
    fn a_directory_named_like_the_marker_hides_its_skills_too() {
        let files = [
            ("first/Symposium.toml", SKILLS.to_owned()),
            ("first/skills/copy/.symposium/kept", String::new()),
            ("first/skills/copy/SKILL.md", skill_md("copied")),
        ];
        let (_root, task) = expecting_skills(&["first"], r#"installed = ["copied"]"#, &files);

        assert_eq!(
            messages(&task.unwrap_err()),
            ["expected skill `copied` is not provided by any of the task's plugins"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_anywhere_in_a_plugin_is_an_error() {
        use std::os::unix::fs::symlink;

        let files = [
            ("real/skills/linked/SKILL.md", skill_md("linked")),
            ("real/run.sh", String::new()),
            ("first/Symposium.toml", SKILLS.to_owned()),
            ("second/Symposium.toml", String::new()),
            ("second/skills/kept/SKILL.md", skill_md("kept")),
        ];
        let (root, _) = expecting_skills(&["first", "second", "bare"], "installed = []", &files);
        let plugins = under(&root, "plugins");
        // A source root, a file outside any source, and a bare plugin's `SKILL.md`:
        // `copy_tree` refuses each when creating a trial.
        symlink(plugins.join("real/skills"), plugins.join("first/skills")).unwrap();
        fs::create_dir_all(plugins.join("second/scripts")).unwrap();
        symlink(
            plugins.join("real/run.sh"),
            plugins.join("second/scripts/run.sh"),
        )
        .unwrap();
        fs::create_dir_all(plugins.join("bare")).unwrap();
        symlink(
            plugins.join("real/skills/linked/SKILL.md"),
            plugins.join("bare/SKILL.md"),
        )
        .unwrap();

        let task = load_demo(&Evals::new(root.path()));

        assert_eq!(
            messages(&task.unwrap_err()),
            [
                "plugin `first` holds a symlink at `skills`, which a trial cannot copy",
                "plugin `second` holds a symlink at `scripts/run.sh`, which a trial cannot copy",
                "plugin `bare` holds a symlink at `SKILL.md`, which a trial cannot copy",
            ]
        );
    }

    #[test]
    fn a_manifest_symposium_rejects_provides_no_skill() {
        // A hook needs a `command`, so Symposium rejects the whole plugin.
        let manifest = format!("{SKILLS}[[hooks]]\nname = \"on-stop\"\nevent = \"Stop\"\n");
        let files = [
            ("first/Symposium.toml", manifest),
            ("first/skills/kept/SKILL.md", skill_md("kept")),
        ];
        let (_root, task) = expecting_skills(&["first"], r#"installed = ["kept"]"#, &files);

        assert_eq!(
            messages(&task.unwrap_err()),
            ["invalid `Symposium.toml` in plugin `first`"]
        );
    }

    #[test]
    fn the_repositorys_tasks_find_their_skills() {
        let evals = Evals::in_repository();
        for (task, skill, plugin) in [
            ("formatting", "rust-formatting", "format-on-stop"),
            ("store-durability", "fixture-store", "fixture-store"),
        ] {
            let task = Task::load(&evals, &TaskName::new(task).unwrap()).unwrap();
            let plugin = evals.plugin_dir(&PluginName::new(plugin).unwrap());

            assert_eq!(
                task.skill_source(&skill_name(skill)),
                Some(plugin.join("skills").join(skill).as_path())
            );
        }
    }

    /// The `demo` task expecting `hooks`, with each of `plugins` declaring its manifest.
    fn expecting_hooks(hooks: &str, plugins: &[(&str, &str)]) -> Result<Task, TaskErrors> {
        let names: Vec<_> = plugins
            .iter()
            .map(|(name, _)| format!("\"{name}\""))
            .collect();
        let (root, evals) = evals_with_task(&format!(
            "kind = \"regression\"\nprompt = \"x\"\nplugins = [{}]\n[expected]\nhooks = {hooks}\n",
            names.join(", ")
        ));
        for (name, manifest) in plugins {
            let dir = root.path().join("plugins").join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("Symposium.toml"), manifest).unwrap();
        }
        load_demo(&evals)
    }

    const ON_STOP: &str = "[[hooks]]\nname = \"on-stop\"\nevent = \"Stop\"\ncommand = \"run\"\n";

    #[test]
    fn an_expected_hook_takes_its_event_from_the_plugin_declaring_it() {
        let task = expecting_hooks(
            r#"["on-stop"]"#,
            &[("first", "name = \"first\"\n"), ("second", ON_STOP)],
        )
        .unwrap();

        assert_eq!(
            task.expected_hooks(),
            [ExpectedHook {
                name: HookName::new("on-stop").unwrap(),
                event: HookEvent::Stop,
            }]
        );
    }

    #[test]
    fn an_expected_hook_declared_twice_is_ambiguous() {
        let errors = expecting_hooks(r#"["on-stop"]"#, &[("first", ON_STOP), ("second", ON_STOP)])
            .unwrap_err();

        assert_eq!(
            messages(&errors),
            ["expected hook `on-stop` is declared by both `first` and `second`"]
        );
    }

    #[test]
    fn an_expected_hook_cannot_be_limited_to_one_agent() {
        let limits = ["agent = \"claude\"", "format = \"codex\""];
        for limit in limits {
            let manifest = format!("{ON_STOP}{limit}\n");

            let errors = expecting_hooks(r#"["on-stop"]"#, &[("plugin", &manifest)]).unwrap_err();

            assert_eq!(
                messages(&errors),
                [
                    "expected hook `on-stop` in plugin `plugin` is limited to one agent by `agent` or `format`, but a task runs on every client"
                ],
                "{limit}"
            );
        }
        let portable = format!("{ON_STOP}format = \"symposium\"\n");
        assert!(expecting_hooks(r#"["on-stop"]"#, &[("plugin", &portable)]).is_ok());
    }

    #[test]
    fn an_expected_hook_must_run_on_a_symposium_event() {
        // The schema holds a hook to Symposium's events.
        let manifest = ON_STOP.replace("Stop", "stop");

        let errors = expecting_hooks(r#"["on-stop"]"#, &[("plugin", &manifest)]).unwrap_err();

        assert_eq!(
            messages(&errors),
            ["invalid `Symposium.toml` in plugin `plugin`"]
        );
    }

    #[test]
    fn a_manifest_that_does_not_parse_leaves_every_expected_hook_undecided() {
        // The broken manifest could declare `on-stop` too, so neither "undeclared" nor
        // "declared once" can be concluded.
        let plugins = [
            (
                "broken",
                "[[hooks]
",
            ),
            ("working", ON_STOP),
        ];

        let errors = expecting_hooks(r#"["on-stop", "other"]"#, &plugins).unwrap_err();

        assert_eq!(
            messages(&errors),
            ["invalid `Symposium.toml` in plugin `broken`"]
        );
    }

    #[test]
    fn the_formatting_tasks_hook_runs_on_stop() {
        let evals = Evals::in_repository();

        let task = Task::load(&evals, &TaskName::new("formatting").unwrap()).unwrap();

        assert_eq!(task.expected_hooks()[0].event, HookEvent::Stop);
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
    fn a_starter_test_cannot_share_a_hidden_checks_name() {
        let (root, evals) = evals_with_task("kind = \"regression\"\nprompt = \"x\"\n");
        let tests = root.path().join("tasks/demo/starter/tests");
        fs::create_dir_all(&tests).unwrap();
        fs::write(tests.join("works.rs"), "").unwrap();

        let errors = load_demo(&evals).unwrap_err();

        assert_eq!(
            messages(&errors),
            [
                "`starter/tests/works.rs` has the name of the hidden check `works`, which grading puts there"
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
