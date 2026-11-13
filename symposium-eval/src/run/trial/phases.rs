//! Each phase of a trial, from a new root to graded code and decided expectations.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{Driver, Ran, Sandbox, Stopped};
use crate::client::{ClientRunner, Ended, Execution, Load, execute};
use crate::expect::config::{ConfigChange, Snapshot};
use crate::expect::decide::{self, Outside, SymposiumTrial};
use crate::expect::{SkillUse, ownership, skill_use};
use crate::grade::{self, GradeError, GradeLimits, Grading, Submitted};
use crate::result::SetupStep;
use crate::run::allocate::NewTrial;
use crate::run::client::Client;
use crate::run::{Configuration, Policy, RunError, Trial};
use crate::task::SkillName;

/// A trial ready to launch.
struct Launch<B> {
    backend: B,
    env: Vec<(OsString, OsString)>,
    /// The client's and Symposium's config as setup left it.
    snapshot: Snapshot,
}

impl Stopped {
    fn at(step: SetupStep) -> impl Fn(RunError) -> Self + Copy {
        move |error| Self { step, error }
    }
}

impl<S: Sandbox> Driver<'_, S> {
    /// Runs each phase in turn, until one stops the trial or its code is graded.
    pub(super) fn ran(&self, new: NewTrial<'_>, runner: &impl ClientRunner) -> Ran {
        let client = runner.client();
        let trial = match new.prepare() {
            Ok(trial) => trial,
            Err(failed) => {
                return Ran::Setup(Stopped {
                    step: SetupStep::Setup,
                    error: failed.into_error(),
                });
            }
        };
        let launch = match self.ready(&trial, client) {
            Ok(launch) => launch,
            Err(stopped) => return Ran::Setup(stopped),
        };
        let task = self.inputs.task();
        let execution = execute(
            runner,
            &launch.backend,
            &trial,
            launch.env,
            task.prompt(),
            &decide::installed_skills(&trial, client, task),
            self.provenance.deadlines.agent,
        );
        let changes = launch.snapshot.changes();
        if matches!(execution.ended, Ended::NotLaunched(_)) {
            return Ran::Session { execution, changes };
        }
        self.graded(&trial, client, execution, changes)
    }

    /// Writes the trial's policy, sets up Symposium for a Symposium trial, and runs the
    /// checks before launch.
    fn ready(&self, trial: &Trial, client: Client) -> Result<Launch<S::Backend<'_>>, Stopped> {
        let setup = Stopped::at(SetupStep::Setup);
        self.write_policy(trial).map_err(setup)?;
        let backend = self.sandbox.for_trial(trial).map_err(setup)?;
        let env = trial
            .environment(self.machine, self.api_key)
            .map_err(setup)?;
        if trial.configuration() == Configuration::Symposium {
            let deadline = self.provenance.deadlines.setup;
            trial
                .set_up_symposium(client, &backend, env.clone(), deadline)
                .map_err(setup)?;
        }
        let snapshot = Snapshot::take(&trial.home(), &trial.project(), client);
        trial
            .check_before_launch(client, &env)
            .map_err(Stopped::at(SetupStep::Prelaunch))?;
        Ok(Launch {
            backend,
            env,
            snapshot,
        })
    }

    fn write_policy(&self, trial: &Trial) -> Result<(), RunError> {
        let policy = Policy::for_trial(trial, self.machine, self.api_host, self.api_key)?;
        trial.write_policy(&policy)
    }

    /// Grades what the session left, after reading what Symposium did.
    fn graded(
        &self,
        trial: &Trial,
        client: Client,
        execution: Execution,
        changes: Vec<ConfigChange>,
    ) -> Ran {
        let task = self.inputs.task();
        let symposium = SymposiumTrial::new(trial);
        let facts = symposium.map(|symposium| {
            decide::gather(symposium, client, task, |skill| self.source_of(skill))
        });
        let copies = facts.as_ref().map_or_else(Vec::new, |facts| {
            ownership::symposium_copies(&trial.project(), &facts.installs, |skill| {
                self.source_of(skill)
            })
        });
        let grading = self.grade(trial, &copies);
        let expectations = symposium.zip(facts).map(|(symposium, facts)| {
            let outside = Outside {
                hooks: &execution.hooks,
                activity: &execution.activity,
                changes: &changes,
            };
            decide::decide(symposium, client, facts, outside, self.uses(&execution))
        });
        Ran::Graded {
            execution,
            grading,
            changes,
            expectations,
        }
    }

    /// Grades `trial`'s project, leaving out `copies`, Symposium's own skill copies.
    fn grade(&self, trial: &Trial, copies: &[String]) -> Grading {
        let prepared = trial
            .prepare_grading(self.machine)
            .and_then(|dirs| Ok((dirs, self.sandbox.for_grading(trial)?)));
        let (dirs, backend) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return Grading::Infrastructure {
                    error: GradeError::Sandbox(Box::new(error)),
                    partial: None,
                };
            }
        };
        let limits = GradeLimits {
            commands: self.provenance.deadlines.grading,
            ..GradeLimits::default()
        };
        grade::grade(
            self.inputs.evals(),
            self.inputs.task(),
            Submitted::trial(&trial.project(), copies),
            &backend,
            &limits,
            &dirs,
        )
    }

    /// The `used` and `not_used` verdicts, from the loads the session showed for each
    /// skill, in the order it looked for them.
    fn uses(&self, execution: &Execution) -> Vec<SkillUse> {
        let expected = self.inputs.task().expected_skills();
        skill_use(expected, |skill| {
            expected
                .names()
                .position(|name| name == skill)
                .and_then(|index| execution.loads.get(index).copied())
                .unwrap_or(Load::Unknown)
        })
    }

    fn source_of(&self, skill: &SkillName) -> Option<PathBuf> {
        self.inputs.task().skill_source(skill).map(Path::to_owned)
    }
}
