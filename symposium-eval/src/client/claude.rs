//! The Claude Code adapter (RFD reference, "Running the client").
//!
//! The adapter builds the whole command line: several valid flags, such as `--bare` or
//! `--no-session-persistence`, would skip hooks and skills or drop the session's own
//! file, and so invalidate a trial. How its transcript is read is in [`transcript`].

mod loads;
mod transcript;
pub(crate) mod version;

use std::ffi::OsString;
use std::path::Path;

use uuid::Uuid;

use self::version::Version;
use super::{ClientRunner, InstalledSkill, Reading};
use crate::grade::process::Invocation;
use crate::run::client::Client;
use crate::run::{Machine, RunError, Trial};

/// One Claude Code session, running one model.
#[derive(Debug, Clone)]
pub(crate) struct Claude {
    model: String,
    /// Names the session's own transcript under the trial's home, which survives a
    /// timeout, and lets the runner check that the stream is this session's.
    session: Uuid,
    /// What preflight found the staged client to be.
    version: Version,
    /// The arguments that set how Claude Code confines the agent.
    sandbox: Vec<OsString>,
}

impl Claude {
    /// For an enforced trial, of the `version` preflight found: `srt` is the boundary,
    /// so Claude Code asks no permission.
    pub(crate) fn enforced(model: String, version: Version) -> Self {
        Self::new(
            model,
            version,
            vec!["--permission-mode".into(), "bypassPermissions".into()],
        )
    }

    /// For an exploratory trial, confined by Claude Code's own sandbox, after writing the
    /// trial's settings file.
    ///
    /// # Errors
    ///
    /// As [`Trial::exploratory_arguments`].
    pub(crate) fn exploratory(
        model: String,
        version: Version,
        trial: &Trial,
        machine: &Machine,
    ) -> Result<Self, RunError> {
        let sandbox = trial.exploratory_arguments(Client::ClaudeCode, machine)?;
        Ok(Self::new(model, version, sandbox))
    }

    fn new(model: String, version: Version, sandbox: Vec<OsString>) -> Self {
        Self {
            model,
            session: Uuid::new_v4(),
            version,
            sandbox,
        }
    }

    pub(crate) fn session(&self) -> Uuid {
        self.session
    }
}

impl ClientRunner for Claude {
    fn invocation(
        &self,
        trial: &Trial,
        prompt: &str,
        env: Vec<(OsString, OsString)>,
    ) -> Invocation {
        let mut args: Vec<OsString> = vec![
            "-p".into(),
            "--output-format".into(),
            "stream-json".into(),
            // Print mode refuses `stream-json` without it.
            "--verbose".into(),
            // Without it, hooks other than `SessionStart`, such as `Stop`, are left out.
            "--include-hook-events".into(),
            "--session-id".into(),
            self.session.to_string().into(),
            "--model".into(),
            self.model.clone().into(),
        ];
        args.extend(self.sandbox.iter().cloned());
        // So a prompt that starts with `-` is not read as an option.
        args.extend(["--".into(), prompt.into()]);
        Invocation {
            program: trial.bin().join("claude").into(),
            args,
            dir: trial.project(),
            env,
            log_name: "claude".to_owned(),
        }
    }

    fn read(&self, trial: &Trial, transcript: &Path, skills: &[InstalledSkill]) -> Reading {
        transcript::read(
            transcript,
            &trial.project(),
            self.session,
            self.version,
            skills,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Configuration;
    use crate::run::test_support::{Host, trial};

    /// Valid flags that would invalidate a trial (research notes, Step 4): they skip hooks
    /// or skills, ignore the trial's settings, drop the session's file, or add partial
    /// events the parser does not read.
    const FORBIDDEN: [&str; 7] = [
        "--bare",
        "--safe-mode",
        "--disable-slash-commands",
        "--restricted",
        "--setting-sources",
        "--no-session-persistence",
        "--include-partial-messages",
    ];

    fn model() -> String {
        "example-model".to_owned()
    }

    #[test]
    fn an_enforced_session_asks_no_permission_and_ends_with_the_prompt() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let claude = Claude::enforced(model(), version::MINIMUM);

        let invocation = claude.invocation(&trial, "-do the task", Vec::new());

        assert_eq!(invocation.program, trial.bin().join("claude"));
        assert_eq!(invocation.dir, trial.project());
        assert_eq!(
            invocation.args,
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-hook-events",
                "--session-id",
                &claude.session().to_string(),
                "--model",
                "example-model",
                "--permission-mode",
                "bypassPermissions",
                "--",
                "-do the task",
            ]
        );
    }

    #[test]
    fn an_exploratory_session_uses_the_trials_own_sandbox() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let claude =
            Claude::exploratory(model(), version::MINIMUM, &trial, &host.machine()).unwrap();

        let invocation = claude.invocation(&trial, "do the task", Vec::new());

        let sandbox = trial
            .exploratory_arguments(Client::ClaudeCode, &host.machine())
            .unwrap();
        assert_eq!(invocation.args[9..invocation.args.len() - 2], sandbox);
    }

    #[test]
    fn each_session_has_its_own_id() {
        assert_ne!(
            Claude::enforced(model(), version::MINIMUM).session(),
            Claude::enforced(model(), version::MINIMUM).session()
        );
    }

    #[test]
    fn no_session_passes_a_flag_that_invalidates_the_trial() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);
        let sessions = [
            Claude::enforced(model(), version::MINIMUM),
            Claude::exploratory(model(), version::MINIMUM, &trial, &host.machine()).unwrap(),
        ];

        for claude in sessions {
            let invocation = claude.invocation(&trial, "do the task", Vec::new());
            let options = &invocation.args[..invocation.args.len() - 1];
            for flag in FORBIDDEN {
                assert!(
                    options.iter().all(|arg| arg != flag),
                    "{flag} in {options:?}"
                );
            }
        }
    }
}
