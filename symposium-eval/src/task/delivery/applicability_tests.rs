//! Exercise task validation and compare accepted predictions with the real compiler.

use std::fs;

use tempfile::TempDir;

use crate::evals::Evals;
use crate::task::{PluginName, Task, TaskError, TaskErrors, TaskName};

#[derive(Clone, Copy)]
enum Filtering {
    WholePlugin,
    SiblingSkill,
}

struct Scenario {
    _dir: TempDir,
    evals: Evals,
    #[cfg(target_os = "linux")]
    filtering: Filtering,
}

impl Scenario {
    fn new(second_name: &str, filtering: Filtering) -> Self {
        let dir = TempDir::new().unwrap();
        let evals = Evals::new(dir.path().to_owned());
        write_task(&evals);
        write_plugin(&evals, "first", "shared-name");
        write_plugin(&evals, "second", second_name);
        write_skill(&evals, "first", "first-skill", "");
        let missing = "predicates: env(SYMPOSIUM_EVAL_DELIVERY_NEVER_SET)\n";
        match filtering {
            Filtering::WholePlugin => write_skill(&evals, "second", "second-skill", missing),
            Filtering::SiblingSkill => {
                write_skill(&evals, "second", "second-skill", "");
                write_skill(&evals, "first", "filtered-sibling", missing);
            }
        }
        Self {
            _dir: dir,
            evals,
            #[cfg(target_os = "linux")]
            filtering,
        }
    }

    fn colliding(filtering: Filtering) -> Self {
        Self::new("shared_name", filtering)
    }

    fn task(&self) -> Result<Task, TaskErrors> {
        Task::load(&self.evals, &TaskName::new("delivery").unwrap())
    }

    fn assert_collision_rejected(&self) {
        let errors = self
            .task()
            .expect_err("conditional bundle-name collisions must be rejected");
        assert_collision_error(&errors);
    }
}

fn write_task(evals: &Evals) {
    let task = TaskName::new("delivery").unwrap();
    let directory = evals.task_dir(&task);
    fs::create_dir_all(&directory).unwrap();
    let original = Evals::in_repository().task_dir(&TaskName::new("formatting").unwrap());
    for component in ["starter", "checks"] {
        crate::grade::copy_exact(
            &original.join(component),
            &directory.join(component),
            "delivery fixture",
        )
        .unwrap();
    }
    fs::write(directory.join("task.toml"),
        "kind = \"regression\"\nprompt = \"Use first-skill.\"\nplugins = [\"first\", \"second\"]\n[expected.skills]\ninstalled = [\"first-skill\"]\n").unwrap();
}

fn write_plugin(evals: &Evals, folder: &str, name: &str) {
    let directory = evals.plugin_dir(&PluginName::new(folder).unwrap());
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("Symposium.toml"),
        format!("name = {name:?}\ndepends-on = [\"*\"]\n[[skills]]\nsource.path = \"skills\"\n"),
    )
    .unwrap();
}

fn write_skill(evals: &Evals, plugin: &str, name: &str, predicates: &str) {
    let directory = evals
        .plugin_dir(&PluginName::new(plugin).unwrap())
        .join("skills")
        .join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("SKILL.md"), format!(
        "---\nname: {name}\ndescription: Delivery fixture.\ndepends-on: '*'\n{predicates}---\n{name}\n"
    )).unwrap();
}

fn assert_collision_error(errors: &TaskErrors) {
    assert!(
        matches!(errors.0.as_slice(), [TaskError::AmbiguousBundleName { name, .. }] if name == "shared-name"),
        "{errors}"
    );
}

#[test]
fn a_filtered_plugin_cannot_silently_change_a_collision_prediction() {
    Scenario::colliding(Filtering::WholePlugin).assert_collision_rejected();
}

#[test]
fn a_filtered_sibling_cannot_silently_change_a_collision_suffix() {
    Scenario::colliding(Filtering::SiblingSkill).assert_collision_rejected();
}

#[cfg(target_os = "linux")]
mod real {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::*;
    use crate::expect::decide;
    use crate::run::client::Client;
    use crate::run::stage::{Audience, Tool};
    use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};
    use crate::run::{Configuration, Trial};

    struct Compiled {
        _real: RealSrt,
        trial: Trial,
        delivered: PathBuf,
    }

    impl Scenario {
        fn compile(&self) -> Compiled {
            let mut real = RealSrt::new(api_key());
            real.stage(
                "cargo-agents",
                &Tool::Executable(built_cargo_agents()),
                Audience::Symposium,
            );
            // A valid trial supplies the sandbox policy even for a fixture task that
            // validation must reject. This keeps the compiler comparison independent.
            let trial = real.trial_of(
                &TaskName::new("formatting").unwrap(),
                1,
                Configuration::Symposium,
            );
            self.copy_plugins(&trial.registry());
            real.set_up_symposium(&trial, Client::ClaudeCode);
            let finished = real.run(&trial, "cargo-agents sync", &[], Duration::from_secs(60));
            assert!(finished.succeeded(), "{}", finished.stderr.tail());
            let delivered =
                delivered_skill(&trial, "first-skill").expect("compiler delivers active skill");
            self.assert_filtering(&trial);
            Compiled {
                _real: real,
                trial,
                delivered,
            }
        }

        fn copy_plugins(&self, registry: &Path) {
            for folder in ["first", "second"] {
                let source = self.evals.plugin_dir(&PluginName::new(folder).unwrap());
                crate::grade::copy_exact(&source, &registry.join(folder), "registry fixture")
                    .unwrap();
            }
        }

        fn assert_filtering(&self, trial: &Trial) {
            let filtered = match self.filtering {
                Filtering::WholePlugin => "second-skill",
                Filtering::SiblingSkill => "filtered-sibling",
            };
            assert!(
                delivered_skill(trial, filtered).is_none(),
                "predicate must filter {filtered}"
            );
            if matches!(self.filtering, Filtering::SiblingSkill) {
                assert!(
                    delivered_skill(trial, "second-skill").is_some(),
                    "the other colliding plugin stays applicable"
                );
            }
        }

        fn assert_path_matches(&self, compiled: &Compiled, task: &Task) {
            let predicted = decide::installed_skills(&compiled.trial, Client::ClaudeCode, task);
            assert_eq!(predicted[0].skill_md, compiled.delivered);
        }

        fn assert_prediction_or_rejection(&self, compiled: &Compiled) {
            match self.task() {
                Ok(task) => self.assert_path_matches(compiled, &task),
                Err(errors) => assert_collision_error(&errors),
            }
        }
    }

    fn delivered_skill(trial: &Trial, skill: &str) -> Option<PathBuf> {
        let skills = trial.project().join(".claude/skills");
        let mut matches = fs::read_dir(skills)
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .join("skills")
                    .join(skill)
                    .join("SKILL.md")
            })
            .filter(|path| path.is_file());
        let found = matches.next();
        assert!(
            matches.next().is_none(),
            "one actual delivery per fixture skill"
        );
        found
    }

    #[test]
    #[ignore = "needs node, srt, and bubblewrap, and builds cargo-agents"]
    fn filtered_plugin_prediction_matches_real_delivery_or_is_rejected() {
        let scenario = Scenario::colliding(Filtering::WholePlugin);
        let compiled = scenario.compile();

        scenario.assert_prediction_or_rejection(&compiled);
    }

    #[test]
    #[ignore = "needs node, srt, and bubblewrap, and builds cargo-agents"]
    fn filtered_sibling_prediction_matches_real_delivery_or_is_rejected() {
        let scenario = Scenario::colliding(Filtering::SiblingSkill);
        let compiled = scenario.compile();

        scenario.assert_prediction_or_rejection(&compiled);
    }

    #[test]
    #[ignore = "needs node, srt, and bubblewrap, and builds cargo-agents"]
    fn distinct_names_preserve_exact_delivery_with_predicate_filtering() {
        for filtering in [Filtering::WholePlugin, Filtering::SiblingSkill] {
            let scenario = Scenario::new("other-plugin", filtering);
            let task = scenario
                .task()
                .expect("distinct bundle names support predicates");
            let compiled = scenario.compile();

            scenario.assert_path_matches(&compiled, &task);
        }
    }
}
