//! The checks just before a client launches (RFD reference, "Preparing a trial").
//!
//! Every trial starts with no skill installed and no managed settings for its client,
//! since either would change what the agent sees before Symposium does anything. A
//! baseline trial must also have no way to reach Symposium: no `cargo-agents` on its
//! `PATH`, no Symposium config, and nothing in the client's configuration that runs
//! `cargo-agents`. A failed check is a setup failure.
//!
//! Each check reads the host. That is what the sandbox sees: it mounts the host's root
//! read-only, and the trial's own directories as they are.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::client::Client;
use super::{Configuration, RunError, Trial, Unready};

/// How a configuration file names Symposium's hook command, by either spelling.
const SYMPOSIUM_COMMANDS: [&str; 2] = ["cargo-agents", "cargo agents"];

impl Trial {
    /// Checks that this trial is ready for `client` to launch with `env`.
    ///
    /// # Errors
    ///
    /// [`RunError::NotReady`] naming the first check that fails, or
    /// [`RunError::Inspect`] when a path cannot be examined.
    pub(crate) fn check_before_launch(
        &self,
        client: Client,
        env: &[(OsString, OsString)],
    ) -> Result<(), RunError> {
        self.check_before_launch_on(Path::new("/"), client, env)
    }

    /// As [`Self::check_before_launch`], with `system` as the machine's root.
    fn check_before_launch_on(
        &self,
        system: &Path,
        client: Client,
        env: &[(OsString, OsString)],
    ) -> Result<(), RunError> {
        for root in [self.project(), self.home()] {
            require_empty(&root.join(client.skill_dir()), Unready::SkillInstalled)?;
        }
        // A trial starts with no record and no trace, so any found afterwards is the
        // session's.
        require_absent(&self.sync_records(), Unready::SyncRecord)?;
        require_absent(&self.hook_trace(), Unready::HookTrace)?;
        require_absent(&self.hook_records(), Unready::HookRecord)?;
        require_empty(&system.join(client.managed_dir()), Unready::ManagedSettings)?;
        if self.configuration() == Configuration::Baseline {
            self.check_no_symposium(client, env)?;
        }
        Ok(())
    }

    fn check_no_symposium(
        &self,
        client: Client,
        env: &[(OsString, OsString)],
    ) -> Result<(), RunError> {
        for directory in path_entries(env) {
            require_absent(&directory.join("cargo-agents"), Unready::CargoAgentsOnPath)?;
        }
        require_absent(&self.home().join(".symposium"), Unready::SymposiumConfig)?;
        let home = client
            .home_config()
            .iter()
            .map(|file| self.home().join(file));
        let project = client
            .project_config()
            .iter()
            .map(|file| self.project().join(file));
        for file in home.chain(project) {
            if runs_symposium(&file)? {
                return Err(not_ready(file, Unready::SymposiumEntry));
            }
        }
        Ok(())
    }
}

/// The directories `env`'s `PATH` lists.
fn path_entries(env: &[(OsString, OsString)]) -> Vec<PathBuf> {
    env.iter()
        .find(|(name, _)| name == "PATH")
        .map(|(_, path)| std::env::split_paths(path).collect())
        .unwrap_or_default()
}

/// Fails with `why` when `directory` holds anything. A missing one holds nothing.
fn require_empty(directory: &Path, why: Unready) -> Result<(), RunError> {
    match fs::read_dir(directory) {
        Ok(mut entries) => match entries.next() {
            Some(_) => Err(not_ready(directory, why)),
            None => Ok(()),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(inspect(directory, source)),
    }
}

/// Fails with `why` when anything is at `path`, even a broken link.
fn require_absent(path: &Path, why: Unready) -> Result<(), RunError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(not_ready(path, why)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(inspect(path, source)),
    }
}

/// Whether `file` exists and names Symposium's command anywhere.
fn runs_symposium(file: &Path) -> Result<bool, RunError> {
    match fs::read(file) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            Ok(SYMPOSIUM_COMMANDS
                .iter()
                .any(|command| text.contains(command)))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(source) => Err(inspect(file, source)),
    }
}

fn not_ready(path: impl Into<PathBuf>, why: Unready) -> RunError {
    RunError::NotReady {
        path: path.into(),
        why,
    }
}

fn inspect(path: &Path, source: std::io::Error) -> RunError {
    RunError::Inspect {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::test_support::{Host, trial};

    const CLIENTS: [Client; 2] = [Client::ClaudeCode, Client::Codex];
    /// The project, then the home.
    const ROOTS: [fn(&Trial) -> PathBuf; 2] = [Trial::project, Trial::home];

    /// A machine with no managed settings, and a trial of `configuration` on it.
    struct Setup {
        host: Host,
        trial: Trial,
    }

    impl Setup {
        fn new(configuration: Configuration) -> Self {
            let host = Host::new();
            fs::create_dir_all(host.join("system/etc")).unwrap();
            let trial = trial(&host.batch(), 1, configuration);
            Self { host, trial }
        }

        fn check(&self, client: Client) -> Result<(), RunError> {
            self.check_with(client, &[])
        }

        fn check_with(&self, client: Client, env: &[(OsString, OsString)]) -> Result<(), RunError> {
            let system = self.host.join("system");
            self.trial.check_before_launch_on(&system, client, env)
        }
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn assert_unready(result: &Result<(), RunError>, expected: Unready) {
        assert!(
            matches!(result, Err(RunError::NotReady { why, .. }) if *why == expected),
            "{result:?}"
        );
    }

    #[test]
    fn a_new_trial_is_ready() {
        for configuration in [Configuration::Symposium, Configuration::Baseline] {
            let setup = Setup::new(configuration);
            for client in CLIENTS {
                setup.check(client).unwrap();
            }
        }
    }

    #[test]
    fn no_skill_is_installed_before_launch() {
        for client in CLIENTS {
            for root in ROOTS {
                let setup = Setup::new(Configuration::Symposium);
                let skill = root(&setup.trial)
                    .join(client.skill_dir())
                    .join("example/SKILL.md");
                write(&skill, "---\nname: example\n---\n");

                assert_unready(&setup.check(client), Unready::SkillInstalled);
            }
        }
    }

    #[test]
    fn no_sync_is_recorded_before_launch() {
        for configuration in [Configuration::Symposium, Configuration::Baseline] {
            let setup = Setup::new(configuration);
            fs::create_dir_all(setup.trial.sync_records()).unwrap();

            assert_unready(&setup.check(Client::ClaudeCode), Unready::SyncRecord);
        }
    }

    #[test]
    fn no_hook_has_run_before_launch() {
        for configuration in [Configuration::Symposium, Configuration::Baseline] {
            let setup = Setup::new(configuration);
            write(&setup.trial.hook_trace(), "");

            assert_unready(&setup.check(Client::ClaudeCode), Unready::HookTrace);
        }
    }

    #[test]
    fn no_plugin_hook_has_recorded_before_launch() {
        for configuration in [Configuration::Symposium, Configuration::Baseline] {
            let setup = Setup::new(configuration);
            write(&setup.trial.hook_records(), "");

            assert_unready(&setup.check(Client::ClaudeCode), Unready::HookRecord);
        }
    }

    #[test]
    fn the_client_has_no_managed_settings() {
        for client in CLIENTS {
            let setup = Setup::new(Configuration::Symposium);
            let managed = setup.host.join("system").join(client.managed_dir());
            write(&managed.join("policy"), "{}");

            assert_unready(&setup.check(client), Unready::ManagedSettings);
        }
    }

    /// Another client's managed settings change nothing for this one.
    #[test]
    fn only_the_clients_own_managed_settings_count() {
        let setup = Setup::new(Configuration::Symposium);
        let managed = setup.host.join("system").join(Client::Codex.managed_dir());
        write(&managed.join("requirements.toml"), "");

        setup.check(Client::ClaudeCode).unwrap();
    }

    #[test]
    fn a_baseline_trial_finds_no_cargo_agents_on_path() {
        let setup = Setup::new(Configuration::Baseline);
        let elsewhere = setup.host.join("elsewhere/bin");
        write(&elsewhere.join("cargo-agents"), "");
        let path = std::env::join_paths([setup.trial.bin(), elsewhere]).unwrap();
        let env = [("PATH".into(), path)];

        assert_unready(
            &setup.check_with(Client::ClaudeCode, &env),
            Unready::CargoAgentsOnPath,
        );
    }

    #[test]
    fn a_baseline_trial_has_no_symposium_config() {
        let setup = Setup::new(Configuration::Baseline);
        write(&setup.trial.symposium_config(), "");

        assert_unready(&setup.check(Client::ClaudeCode), Unready::SymposiumConfig);
    }

    #[test]
    fn a_baseline_clients_configuration_never_runs_cargo_agents() {
        for client in CLIENTS {
            let [project, home] = ROOTS;
            let files = (client.project_config().iter().map(|file| (project, file)))
                .chain(client.home_config().iter().map(|file| (home, file)));
            for (root, file) in files {
                let setup = Setup::new(Configuration::Baseline);
                let config = root(&setup.trial).join(file);
                write(&config, r#"{"command": "cargo-agents hook"}"#);

                assert_unready(&setup.check(client), Unready::SymposiumEntry);
            }
        }
    }

    #[test]
    fn a_baseline_clients_own_configuration_is_fine() {
        let setup = Setup::new(Configuration::Baseline);
        let settings = setup.trial.home().join(".claude/settings.json");
        write(&settings, r#"{"theme": "dark"}"#);

        setup.check(Client::ClaudeCode).unwrap();
    }

    /// On this machine, under real `srt`: a Symposium trial just set up, and a baseline
    /// trial beside it, are both ready. It fails where the machine has managed settings
    /// for Claude Code, as a real run would.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn trials_are_ready_after_a_real_setup() {
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let symposium = real.trial(1, Configuration::Symposium);
        let baseline = real.trial(2, Configuration::Baseline);
        real.set_up_symposium(&symposium, Client::ClaudeCode);

        for trial in [&symposium, &baseline] {
            let configuration = trial.configuration();
            let result = real.check_before_launch(trial, Client::ClaudeCode);
            assert!(result.is_ok(), "{configuration}: {result:?}");
        }
    }

    /// Setting up Symposium is what puts these in a Symposium trial.
    #[test]
    fn a_symposium_trial_may_reach_symposium() {
        let setup = Setup::new(Configuration::Symposium);
        write(&setup.trial.symposium_config(), "");
        let settings = setup.trial.home().join(".claude/settings.json");
        write(&settings, r#"{"command": "cargo-agents hook claude"}"#);
        let env = [("PATH".into(), setup.trial.bin().into_os_string())];
        write(&setup.trial.bin().join("cargo-agents"), "");

        setup.check_with(Client::ClaudeCode, &env).unwrap();
    }
}
