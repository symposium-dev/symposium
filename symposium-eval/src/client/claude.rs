//! The Claude Code adapter (RFD reference, "Running the client").
//!
//! The adapter builds the whole command line: several valid flags, such as `--bare` or
//! `--no-session-persistence`, would skip hooks and skills or drop the session's own
//! file, and so invalidate a trial. How its transcript is read is in [`transcript`].

mod calls;
#[cfg(test)]
mod fixtures;
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
#[derive(Debug)]
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
                let with_value = format!("{flag}=");
                assert!(
                    options.iter().all(|arg| {
                        let arg = arg.to_string_lossy();
                        arg != flag && !arg.starts_with(&with_value)
                    }),
                    "{flag} in {options:?}"
                );
            }
        }
    }

    /// A stand-in `claude` that replays a recorded transcript, run through the real
    /// backend: no model and no API key, but the adapter's command line, supervision,
    /// deadline and transcript reading all take part.
    #[cfg(unix)]
    mod stand_in {
        use std::fs;
        use std::time::Duration;

        use super::*;
        use crate::client::claude::fixtures::{self, MODEL_ERROR, SESSION, SKILL_LOADED};
        use crate::client::{ClientError, Ended, Execution, Load, ProvenanceError, execute};
        use crate::grade::process::Local;
        use crate::run::test_support::executable;
        use crate::task::SkillName;

        /// Which session the stand-in writes its events as.
        enum Speaks {
            AsRequested,
            AsRecorded,
        }

        /// Stages a `claude` that writes `transcript` as `speaks` says, then runs `then`.
        fn stage(trial: &Trial, transcript: &str, speaks: Speaks, then: &str) {
            let recorded = trial.session().join("recorded.jsonl");
            fs::write(&recorded, transcript).unwrap();
            let session = match speaks {
                Speaks::AsRequested => r#""$session""#,
                Speaks::AsRecorded => SESSION,
            };
            fs::create_dir_all(trial.bin()).unwrap();
            executable(
                &trial.bin().join("claude"),
                &format!(
                    r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        --session-id) session=$2; shift ;;
        --) break ;;
    esac
    shift
done
sed "s/{SESSION}/{session}/g" '{}'
{then}
"#,
                    recorded.display()
                ),
            );
        }

        fn run(transcript: &str, speaks: Speaks, then: &str, deadline: Duration) -> Execution {
            let host = Host::new();
            let trial = trial(&host.batch(), 1, Configuration::Symposium);
            stage(&trial, transcript, speaks, then);
            let skill = InstalledSkill {
                name: SkillName::new("fixture-greeting").unwrap(),
                skill_md: trial
                    .project()
                    .join(".claude/skills/fixture-greeting/SKILL.md"),
            };
            execute(
                &Claude::enforced(model(), version::MINIMUM),
                &Local,
                &trial,
                vec![("PATH".into(), "/usr/bin:/bin".into())],
                "do the task",
                &[skill],
                deadline,
            )
        }

        fn before_result(transcript: &str) -> String {
            fixtures::before(transcript, "result")
        }

        #[test]
        fn a_replayed_session_completes_and_loads_its_skill() {
            let execution = run(
                SKILL_LOADED,
                Speaks::AsRequested,
                "exit 0",
                Duration::from_secs(60),
            );

            assert!(matches!(execution.ended, Ended::Completed), "{execution:?}");
            assert_eq!(execution.loads, [Load::Loaded]);
        }

        #[test]
        fn a_client_that_ignores_the_requested_session_is_mismatched() {
            let execution = run(
                SKILL_LOADED,
                Speaks::AsRecorded,
                "exit 0",
                Duration::from_secs(60),
            );

            assert!(
                matches!(
                    &execution.ended,
                    Ended::Mismatched(ProvenanceError::Session { found, .. })
                        if found == SESSION
                ),
                "{execution:?}"
            );
            assert_eq!(execution.loads, [Load::Unknown]);
        }

        #[test]
        fn a_session_past_its_deadline_times_out_and_keeps_its_load() {
            let execution = run(
                &before_result(SKILL_LOADED),
                Speaks::AsRequested,
                "exec sleep 60",
                Duration::from_secs(3),
            );

            assert!(
                matches!(
                    execution.ended,
                    Ended::TimedOut {
                        after_deadline: None
                    }
                ),
                "{execution:?}"
            );
            assert_eq!(execution.loads, [Load::Loaded]);
        }

        #[test]
        fn an_api_error_is_reported_in_claudes_words_whatever_the_exit() {
            for exit in ["exit 0", "exit 1"] {
                let execution = run(
                    MODEL_ERROR,
                    Speaks::AsRequested,
                    exit,
                    Duration::from_secs(60),
                );

                assert!(
                    matches!(&execution.ended, Ended::Failed(ClientError::Reported(message)) if message.contains("not-a-model")),
                    "{execution:?}"
                );
            }
        }

        #[test]
        fn a_client_that_stops_without_a_result_has_no_completion() {
            let execution = run(
                &before_result(SKILL_LOADED),
                Speaks::AsRequested,
                "exit 0",
                Duration::from_secs(60),
            );

            assert!(
                matches!(execution.ended, Ended::Failed(ClientError::NoCompletion)),
                "{execution:?}"
            );
            assert_eq!(execution.loads, [Load::Loaded]);
        }
    }
}
