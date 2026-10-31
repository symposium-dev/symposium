//! Exploratory mode: trials inside each client's own sandbox, where `srt` is unavailable
//! (RFD reference, "Exploratory: without `srt`").
//!
//! The client runs directly, and its sandbox confines only the commands the agent runs:
//! hooks, and with them Symposium, run outside it in both clients, and the API key is in
//! the client's environment. So exploratory results are labeled and never compared.
//!
//! Claude Code's sandbox is built on `srt`, so its commands get the trial's [`Reach`],
//! as an enforced trial does, with no network. That holds from Claude Code 2.1.285: only
//! then does disabling unsandboxed retries through `--settings` stop the repository's
//! settings from loosening the sandbox.
//!
//! Codex can only add writable roots to its workspace sandbox; its reads stay open. The
//! roots are set as `sandbox_workspace_write.writable_roots`, not with `--add-dir`, which
//! does not make them writable on some versions (openai/codex issue 18448).
//!
//! Claude Code's preflight (`client::claude::version`) refuses older releases.
//!
//! FIXME: run that preflight before launch, once clients are launched.
//!
//! Setup and pre-launch checks are as in enforced mode, with `cargo agents init` run
//! through the local backend.
//!
//! FIXME: label the results exploratory, once there are results.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::client::Client;
use super::{Machine, Reach, RunError, Trial};

/// Claude Code's `--settings` file for an exploratory trial.
#[derive(Debug, Serialize)]
struct ClaudeSettings {
    sandbox: Sandbox,
    permissions: Permissions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sandbox {
    enabled: bool,
    /// Exit rather than run commands unsandboxed.
    fail_if_unavailable: bool,
    /// Ignore the agent's request to run a command outside the sandbox.
    allow_unsandboxed_commands: bool,
    filesystem: Filesystem,
    network: Network,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Filesystem {
    deny_read: Vec<PathBuf>,
    allow_read: Vec<PathBuf>,
    allow_write: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Network {
    allowed_domains: Vec<String>,
    /// Deny every other host instead of asking.
    strict_allowlist: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Permissions {
    /// Keeps the file tools, which the sandbox does not cover, in the project.
    block_reads_outside_working_directories: bool,
}

impl Trial {
    /// Where Claude Code's settings for this trial are written: outside every directory
    /// its commands may write, since it reloads a settings file that changes.
    pub(crate) fn claude_settings_file(&self) -> PathBuf {
        self.root().join("claude-settings.json")
    }

    /// The arguments that confine `client` to its own sandbox in this trial, writing
    /// Claude Code's settings file first.
    ///
    /// # Errors
    ///
    /// As [`Reach::for_trial`], and when a path would be read as a pattern or the
    /// settings file cannot be written.
    pub(crate) fn exploratory_arguments(
        &self,
        client: Client,
        machine: &Machine,
    ) -> Result<Vec<OsString>, RunError> {
        match client {
            Client::ClaudeCode => {
                let settings = claude_settings(Reach::for_trial(self, machine)?)?;
                let path = self.claude_settings_file();
                write_json(&settings, &path)?;
                Ok(vec![
                    "--settings".into(),
                    path.into(),
                    "--permission-mode".into(),
                    "acceptEdits".into(),
                ])
            }
            Client::Codex => self.codex_arguments(),
        }
    }

    /// The workspace sandbox, with the trial's other writable directories as roots.
    fn codex_arguments(&self) -> Result<Vec<OsString>, RunError> {
        let mut roots = Vec::new();
        for root in [self.cargo_home(), self.home(), self.evidence()] {
            let text = root
                .to_str()
                .ok_or_else(|| RunError::NotUnicode(root.clone()))?;
            roots.push(toml::Value::from(text));
        }
        // `--config` parses its value as TOML, so the array quotes each path.
        let writable_roots = toml::Value::Array(roots);
        Ok(vec![
            "--sandbox".into(),
            "workspace-write".into(),
            "--config".into(),
            format!("sandbox_workspace_write.writable_roots={writable_roots}").into(),
        ])
    }
}

fn claude_settings(reach: Reach) -> Result<ClaudeSettings, RunError> {
    for path in reach
        .deny_read
        .iter()
        .chain(&reach.allow_read)
        .chain(&reach.allow_write)
    {
        reject_pattern(path)?;
    }
    Ok(ClaudeSettings {
        sandbox: Sandbox {
            enabled: true,
            fail_if_unavailable: true,
            allow_unsandboxed_commands: false,
            filesystem: Filesystem {
                deny_read: reach.deny_read,
                allow_read: reach.allow_read,
                allow_write: reach.allow_write,
            },
            network: Network {
                allowed_domains: Vec::new(),
                strict_allowlist: true,
            },
        },
        permissions: Permissions {
            block_reads_outside_working_directories: true,
        },
    })
}

/// Claude Code reads `*`, `?`, and brackets in its sandbox paths as glob patterns, and
/// has no way to say otherwise. Only names count: a Windows prefix such as `\\?\` is not a pattern.
fn reject_pattern(path: &Path) -> Result<(), RunError> {
    let is_pattern = path.components().any(|component| match component {
        Component::Normal(name) => name.to_string_lossy().contains(['*', '?', '[', ']']),
        _ => false,
    });
    if is_pattern {
        return Err(RunError::PatternInPath(path.to_owned()));
    }
    Ok(())
}

fn write_json(settings: &ClaudeSettings, path: &Path) -> Result<(), RunError> {
    let text = serde_json::to_string_pretty(settings).map_err(RunError::ClientSettings)?;
    fs::write(path, text).map_err(|source| RunError::Create {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::run::Configuration;
    use crate::run::test_support::{Host, trial};

    fn settings_of(trial: &Trial, machine: &Machine) -> Value {
        trial
            .exploratory_arguments(Client::ClaudeCode, machine)
            .unwrap();
        let text = fs::read_to_string(trial.claude_settings_file()).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn paths(value: &Value) -> Vec<PathBuf> {
        let entries = value.as_array().unwrap();
        entries
            .iter()
            .map(|entry| PathBuf::from(entry.as_str().unwrap()))
            .collect()
    }

    #[test]
    fn claude_code_runs_with_its_settings_file_and_accepts_edits() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let arguments = trial
            .exploratory_arguments(Client::ClaudeCode, &host.machine())
            .unwrap();

        let settings = trial.claude_settings_file();
        let expected: [OsString; 4] = [
            "--settings".into(),
            settings.into(),
            "--permission-mode".into(),
            "acceptEdits".into(),
        ];
        assert_eq!(arguments, expected);
    }

    #[test]
    fn claude_codes_sandbox_is_strict_and_offline() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let settings = settings_of(&trial, &host.machine());

        let sandbox = &settings["sandbox"];
        assert_eq!(sandbox["enabled"], true);
        assert_eq!(sandbox["failIfUnavailable"], true);
        assert_eq!(sandbox["allowUnsandboxedCommands"], false);
        assert_eq!(
            sandbox["network"],
            json!({ "allowedDomains": [], "strictAllowlist": true })
        );
        assert_eq!(
            settings["permissions"],
            json!({ "blockReadsOutsideWorkingDirectories": true })
        );
    }

    /// The same rules as the trial's `srt` policy, for each configuration.
    #[test]
    fn claude_codes_commands_get_the_trials_reach() {
        let host = Host::new();
        let batch = host.batch();
        let machine = host.machine();
        for (number, configuration) in [Configuration::Symposium, Configuration::Baseline]
            .into_iter()
            .enumerate()
        {
            let trial = trial(&batch, number + 1, configuration);
            let reach = Reach::for_trial(&trial, &machine).unwrap();

            let filesystem = &settings_of(&trial, &machine)["sandbox"]["filesystem"];

            assert_eq!(paths(&filesystem["denyRead"]), reach.deny_read);
            assert_eq!(paths(&filesystem["allowRead"]), reach.allow_read);
            assert_eq!(paths(&filesystem["allowWrite"]), reach.allow_write);
            let staged = batch.symposium_tools();
            let reaches_staged = paths(&filesystem["allowRead"]).contains(&staged);
            assert_eq!(
                reaches_staged,
                configuration == Configuration::Symposium,
                "{configuration}"
            );
        }
    }

    #[test]
    fn a_path_claude_code_would_read_as_a_pattern_is_refused() {
        let host = Host::new();
        let roots = vec![host.join("home"), host.join("team[1]")];
        let machine = Machine::for_tests(
            roots,
            &host.join("checkout"),
            &host.join("home/dev/.rustup/toolchains/stable"),
        );
        let trial = trial(&host.batch(), 1, Configuration::Baseline);

        let result = trial.exploratory_arguments(Client::ClaudeCode, &machine);

        assert!(
            matches!(result, Err(RunError::PatternInPath(_))),
            "{result:?}"
        );
        assert!(!trial.claude_settings_file().exists());
    }

    #[test]
    fn codex_writes_only_its_workspace_and_the_trials_own_directories() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);

        let arguments = trial
            .exploratory_arguments(Client::Codex, &host.machine())
            .unwrap();

        let [sandbox, mode, config, value] = arguments.as_slice() else {
            panic!("{arguments:?}");
        };
        assert_eq!(
            [sandbox, mode, config],
            ["--sandbox", "workspace-write", "--config"]
        );
        // Parsed back as Codex parses it, so a path that needs quoting is checked too.
        let value = value.to_str().unwrap();
        let (key, roots) = value.split_once('=').unwrap();
        assert_eq!(key, "sandbox_workspace_write.writable_roots");
        let roots: toml::Table = toml::from_str(&format!("roots = {roots}")).unwrap();
        let expected = [trial.cargo_home(), trial.home(), trial.evidence()]
            .map(|root| toml::Value::from(root.to_str().unwrap()));
        assert_eq!(roots["roots"].as_array().unwrap(), &expected);
    }

    /// Braces and parentheses are ordinary characters in Claude Code's sandbox paths.
    #[test]
    fn braces_and_parentheses_are_not_patterns() {
        reject_pattern(Path::new("/srv/{team} (shared)")).unwrap();
    }
}
