//! The Symposium configuration of a trial (RFD reference, "Preparing a trial").
//!
//! A Symposium trial's home gets a config whose only registry is the trial's `registry/`,
//! which holds the task's plugins; the built-in registries are off, and so is
//! auto-update, so nothing is fetched mid-trial. Then the staged `cargo-agents` runs
//! `init` inside the trial's sandbox, as every other command there does. `init` registers
//! hooks without syncing, and globally, in the trial's home: project-scoped hooks exist
//! only after a sync, which would install skills before the behavior under test.
//!
//! A baseline trial is never set up: nothing is uninstalled for it, it simply has no
//! Symposium to reach.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::client::Client;
use super::{Configuration, RunError, Trial, succeeded};
use crate::grade::process::{Backend, Invocation};

/// `~/.symposium/config.toml` for a Symposium trial.
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Config<'a> {
    auto_sync: bool,
    hook_scope: &'static str,
    auto_update: &'static str,
    defaults: Defaults,
    registry: [Registry<'a>; 1],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Defaults {
    symposium_recommendations: bool,
    user_plugins: bool,
}

#[derive(Debug, Serialize)]
struct Registry<'a> {
    name: &'static str,
    path: &'a Path,
}

/// Where Symposium reads its config, relative to the home.
pub(crate) const SYMPOSIUM_CONFIG: &str = ".symposium/config.toml";

impl Trial {
    /// Where Symposium reads its config in this trial.
    pub(crate) fn symposium_config(&self) -> PathBuf {
        self.home().join(SYMPOSIUM_CONFIG)
    }

    /// Writes the trial's Symposium config, then runs the staged `cargo agents init` for
    /// `client` through `backend`, with `env` as its whole environment.
    ///
    /// # Errors
    ///
    /// When the trial is a baseline, the config cannot be written, or `init` does not
    /// succeed within `deadline`.
    pub(crate) fn set_up_symposium(
        &self,
        client: Client,
        backend: &impl Backend,
        env: Vec<(OsString, OsString)>,
        deadline: Duration,
    ) -> Result<(), RunError> {
        if self.configuration() != Configuration::Symposium {
            return Err(RunError::NotSymposium(self.root().to_owned()));
        }
        self.write_symposium_config()?;
        let invocation = self.init_invocation(client, env);
        let finished = backend.run(&invocation, deadline, &self.evidence());
        succeeded("cargo agents init", finished).map(drop)
    }

    fn write_symposium_config(&self) -> Result<(), RunError> {
        let registry = self.registry();
        let config = Config {
            auto_sync: true,
            hook_scope: "global",
            auto_update: "off",
            defaults: Defaults {
                symposium_recommendations: false,
                user_plugins: false,
            },
            registry: [Registry {
                name: "eval",
                path: &registry,
            }],
        };
        let text = toml::to_string(&config).map_err(RunError::SymposiumConfig)?;
        let path = self.symposium_config();
        let directory = path.parent().expect("the config is inside the home");
        fs::create_dir_all(directory).map_err(|source| RunError::Create {
            path: directory.to_owned(),
            source,
        })?;
        fs::write(&path, text).map_err(|source| RunError::Create { path, source })
    }

    /// `cargo-agents init`, found on the trial's `PATH` through its `bin/` link.
    fn init_invocation(&self, client: Client, env: Vec<(OsString, OsString)>) -> Invocation {
        Invocation {
            program: "cargo-agents".into(),
            args: vec![
                "init".into(),
                "--add-agent".into(),
                client.symposium_name().into(),
            ],
            dir: self.project(),
            env,
            log_name: "symposium-init".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grade::process::Local;
    use crate::run::test_support::{Host, trial};

    #[test]
    fn the_config_makes_the_trials_registry_the_only_one() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        trial.write_symposium_config().unwrap();

        assert_eq!(read_config(&trial), expected_config(&trial));
    }

    fn read_config(trial: &Trial) -> toml::Table {
        let text = fs::read_to_string(trial.symposium_config()).unwrap();
        toml::from_str(&text).unwrap()
    }

    /// What the trial's config must hold, before `init` and after it.
    fn expected_config(trial: &Trial) -> toml::Table {
        toml::from_str(&format!(
            r#"
            auto-sync = true
            hook-scope = "global"
            auto-update = "off"

            [defaults]
            symposium-recommendations = false
            user-plugins = false

            [[registry]]
            name = "eval"
            path = {}
            "#,
            toml::Value::from(trial.registry().to_str().unwrap())
        ))
        .unwrap()
    }

    #[test]
    fn init_registers_the_client_from_the_staged_cargo_agents() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let invocation = trial.init_invocation(Client::Codex, Vec::new());

        assert_eq!(invocation.program, "cargo-agents");
        assert_eq!(invocation.args, ["init", "--add-agent", "codex"]);
        assert_eq!(invocation.dir, trial.project());
    }

    /// Runs the `cargo-agents` built from this checkout under real `srt`: run it on Linux
    /// with `cargo test -p symposium-eval -- --ignored`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn a_symposium_trial_registers_hooks_without_syncing() {
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let trial = real.trial(1, Configuration::Symposium);

        real.set_up_symposium(&trial, Client::ClaudeCode);

        assert_isolation_survives_init(&trial);
        let settings = trial.home().join(".claude/settings.json");
        let settings = fs::read_to_string(settings).unwrap();
        assert!(settings.contains("cargo-agents hook claude"), "{settings}");
        // No sync ran, so no skill is installed before the agent starts. The registry's
        // plugin applies to the project, so a sync would have installed one here.
        for skills in [trial.project(), trial.home()].map(|root| root.join(".claude/skills")) {
            let installed = match fs::read_dir(&skills) {
                Ok(entries) => entries.count(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => panic!("cannot read {}: {error}", skills.display()),
            };
            assert_eq!(installed, 0, "{}", skills.display());
        }
    }

    /// What the agent's first hook will do: sync, installing the task's skill in the
    /// project, which the sandbox must allow.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn a_sync_in_a_symposium_trial_installs_the_tasks_skill() {
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let trial = real.trial(1, Configuration::Symposium);
        real.set_up_symposium(&trial, Client::ClaudeCode);

        let finished = real.run(&trial, "cargo-agents sync", &[], Duration::from_secs(60));

        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        let skill = trial
            .project()
            .join(".claude/skills/fixture-store/SKILL.md");
        assert!(skill.is_file(), "{}", finished.stdout.tail());
    }

    /// `init` rewrites the config: it must add the client and keep everything that keeps
    /// the trial offline and on its own registry.
    #[cfg(target_os = "linux")]
    fn assert_isolation_survives_init(trial: &Trial) {
        let mut config = read_config(trial);
        // `init` writes out a setting only when it differs from the default.
        config.entry("hook-scope").or_insert("global".into());
        // A registry's own `auto-update` applies only to a git registry.
        for registry in config["registry"].as_array_mut().unwrap() {
            registry.as_table_mut().unwrap().remove("auto-update");
        }
        for (key, expected) in expected_config(trial) {
            assert_eq!(config.get(&key), Some(&expected), "{key}: {config}");
        }
        let agents = config["agent"].as_array().unwrap();
        let names: Vec<_> = agents.iter().map(|agent| &agent["name"]).collect();
        assert_eq!(names, [&toml::Value::from("claude")], "{config}");
    }

    #[test]
    fn a_baseline_trial_is_never_set_up() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);

        let result = trial.set_up_symposium(
            Client::ClaudeCode,
            &Local,
            Vec::new(),
            Duration::from_secs(1),
        );

        assert!(
            matches!(result, Err(RunError::NotSymposium(_))),
            "{result:?}"
        );
        assert!(!trial.symposium_config().exists());
    }
}
