//! Each way a trial can end, through the driver, with a scripted sandbox: `init` and the
//! client are played, and grading runs for real, unconfined.

use std::cell::Cell;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::client::{
    Completion, HookEvidence, InstalledSkill, Load, Reading, SessionIntroduction, Summary,
    ToolActivity, Usage,
};
use crate::grade::ProcessError;
use crate::grade::process::{Backend, Finished, Invocation, Local};
use crate::result::{Deadlines, IsolationMode, SymposiumBuild};
use crate::run::client::Client;
use crate::run::test_support::{Host, api_key, inputs};
use crate::run::{Configuration, Trial};

/// How the played `cargo-agents init` ends.
#[derive(Clone, Copy)]
enum Init {
    Succeeds,
    Fails,
    /// Times out, then loses certainty that its process tree stopped.
    UncleanTimeout,
    /// Succeeds, but leaves a sync record, as a sync would.
    Syncs,
}

/// What the played client does.
#[derive(Clone, Copy)]
enum Session {
    /// Writes the solution and a finished transcript.
    Solves,
    /// Writes the solution, then runs past its deadline.
    TimesOut,
    CannotStart,
    /// The sandbox starts, but the client exits before introducing a session.
    WrapperExits(i32),
    /// Starts, but the runner loses supervision of its process tree.
    Unsupervised,
    /// Times out, but a descendant keeps an output pipe open.
    UncleanTimeout,
    /// Solves, and writes a `result.json` in the trial's root.
    PlantsResult,
}

struct Scripted {
    init: Init,
    session: Session,
    /// Whether the trial's sandbox can be prepared.
    trial_sandbox: bool,
    /// Whether grading's sandbox can be prepared.
    grading_sandbox: bool,
    /// Whether the build times out without proving its process tree stopped.
    unclean_grading_timeout: bool,
    grading_calls: Cell<usize>,
}

impl Scripted {
    fn new(session: Session) -> Self {
        Self {
            init: Init::Succeeds,
            session,
            trial_sandbox: true,
            grading_sandbox: true,
            unclean_grading_timeout: false,
            grading_calls: Cell::new(0),
        }
    }
}

enum Played<'a> {
    Trial { script: &'a Scripted, trial: Trial },
    Grading { script: &'a Scripted },
}

impl Sandbox for Scripted {
    type Backend<'a> = Played<'a>;

    fn for_trial(&self, trial: &Trial) -> Result<Played<'_>, RunError> {
        if !self.trial_sandbox {
            return Err(RunError::NoPolicy(trial.policy_file()));
        }
        Ok(Played::Trial {
            script: self,
            trial: trial.clone(),
        })
    }

    fn for_grading(&self, trial: &Trial) -> Result<Played<'_>, RunError> {
        self.grading_calls.set(self.grading_calls.get() + 1);
        if self.grading_sandbox {
            Ok(Played::Grading { script: self })
        } else {
            Err(RunError::NoPolicy(trial.grading_policy_file()))
        }
    }
}

impl Backend for Played<'_> {
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError> {
        let (script, trial) = match self {
            Self::Trial { script, trial } => (*script, Some(trial)),
            Self::Grading { script } => (*script, None),
        };
        if trial.is_none() {
            if script.unclean_grading_timeout && invocation.log_name == "build" {
                return Ok(unclean_timeout());
            }
            return Local.run(invocation, deadline, logs);
        }
        let trial = trial.unwrap();
        if invocation.program == "cargo-agents" {
            return Ok(init(script.init, trial));
        }
        fs::write(launched(logs), launched_model(invocation)).unwrap();
        play(script.session, trial, &invocation.log(logs, "stdout"))
    }
}

/// Where the played client keeps the model it was launched with.
fn launched(logs: &Path) -> PathBuf {
    logs.join("launched-model")
}

fn launched_model(invocation: &Invocation) -> String {
    let flag = invocation.args.iter().position(|arg| arg == "--model");
    let model = flag.and_then(|flag| invocation.args.get(flag + 1)).unwrap();
    model.to_str().unwrap().to_owned()
}

fn init(init: Init, trial: &Trial) -> Finished {
    match init {
        Init::Succeeds => exited(0),
        Init::Fails => exited(1),
        Init::UncleanTimeout => unclean_timeout(),
        Init::Syncs => {
            fs::create_dir_all(trial.sync_records()).unwrap();
            exited(0)
        }
    }
}

fn play(session: Session, trial: &Trial, transcript: &Path) -> Result<Finished, ProcessError> {
    if let Session::CannotStart = session {
        return Err(ProcessError::Spawn {
            program: "client".into(),
            source: io::Error::from(io::ErrorKind::NotFound),
        });
    }
    match session {
        Session::WrapperExits(code) => {
            fs::write(transcript, "").unwrap();
            return Ok(exited(code));
        }
        Session::TimesOut => return Ok(Finished::never_started()),
        Session::Unsupervised => return Err(ProcessError::OutputNotClosed),
        Session::UncleanTimeout => {
            let mut finished = Finished::never_started();
            finished.after_deadline = Some(ProcessError::OutputNotClosed);
            return Ok(finished);
        }
        Session::Solves | Session::CannotStart | Session::PlantsResult => {}
    }
    fs::copy(solution(), trial.project().join("src/lib.rs")).unwrap();
    fs::write(transcript, "finished\n").unwrap();
    if let Session::PlantsResult = session {
        fs::write(trial.root().join(RESULT_FILE), "planted").unwrap();
    }
    Ok(exited(0))
}

fn solution() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/tasks/formatting/solution/src/lib.rs")
}

fn exited(code: i32) -> Finished {
    let mut finished = Finished::never_started();
    finished.status = Some(exit_status(code));
    finished
}

fn unclean_timeout() -> Finished {
    let mut finished = Finished::never_started();
    finished.after_deadline = Some(ProcessError::OutputNotClosed);
    finished
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    std::os::unix::process::ExitStatusExt::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    std::os::windows::process::ExitStatusExt::from_raw(code.cast_unsigned())
}

/// A client run as `client`, whose transcript finishes when it says `finished`. It
/// passes its model as `--model`, so a test can see which model was launched.
struct Stub {
    client: Client,
    model: &'static str,
    /// Shows no loads, whatever skills it was asked about.
    loses_loads: bool,
}

impl Stub {
    fn new() -> Self {
        Self {
            client: Client::ClaudeCode,
            model: "a-model",
            loses_loads: false,
        }
    }
}

impl ClientRunner for Stub {
    fn client(&self) -> Client {
        self.client
    }

    fn model(&self) -> &str {
        self.model
    }

    fn invocation(
        &self,
        trial: &Trial,
        prompt: &str,
        env: Vec<(OsString, OsString)>,
    ) -> Invocation {
        Invocation {
            program: "client".into(),
            args: vec!["--model".into(), self.model.into(), prompt.into()],
            dir: trial.project(),
            env,
            log_name: "client".to_owned(),
        }
    }

    fn read(&self, _: &Trial, transcript: &Path, skills: &[InstalledSkill]) -> Reading {
        let finished = fs::read_to_string(transcript).is_ok_and(|text| text == "finished\n");
        let loads = if self.loses_loads { 0 } else { skills.len() };
        Reading {
            introduction: if finished {
                SessionIntroduction::Observed
            } else {
                SessionIntroduction::NotObserved
            },
            completion: Ok(if finished {
                Completion::Finished
            } else {
                Completion::Missing
            }),
            loads: vec![Load::Unknown; loads],
            hooks: HookEvidence::Unsupported,
            activity: ToolActivity::unread(),
            summary: Summary::unread(Usage::Unsupported),
        }
    }

    fn unread_hooks(&self) -> HookEvidence {
        HookEvidence::Unsupported
    }

    fn unread_usage(&self) -> Usage {
        Usage::Unsupported
    }
}

fn provenance() -> Provenance {
    Provenance {
        client_version: Some("1.0.0".to_owned()),
        os: "test".to_owned(),
        toolchain: None,
        isolation: IsolationMode::Enforced,
        srt_version: None,
        deadlines: Deadlines {
            setup: Duration::from_secs(60),
            agent: Duration::from_secs(60),
            grading: Duration::from_secs(300),
        },
    }
}

fn arm(configuration: Configuration) -> Arm {
    match configuration {
        Configuration::Baseline => Arm::Baseline,
        Configuration::Symposium => Arm::Symposium(SymposiumBuild {
            version: "cargo-agents 1.0.0".to_owned(),
            sha256: crate::digest::file(&solution()).unwrap(),
            revision: None,
        }),
    }
}

/// A batch of `formatting` on a scratch machine, to run trials in.
struct Bench {
    _host: Host,
    batch: Batch,
    inputs: Inputs,
    machine: Machine,
    provenance: Provenance,
}

impl Bench {
    fn new() -> Self {
        let host = Host::new();
        let batch = host.batch();
        let inputs = inputs(&batch, "formatting");
        let machine = host.machine();
        Self {
            _host: host,
            batch,
            inputs,
            machine,
            provenance: provenance(),
        }
    }

    fn run(
        &self,
        configuration: Configuration,
        sandbox: &Scripted,
        runner: &Stub,
    ) -> Result<Done, TrialError> {
        let key = api_key();
        let driver = Driver {
            batch: &self.batch,
            inputs: &self.inputs,
            machine: &self.machine,
            sandbox,
            api_host: "api.example.com",
            api_key: &key,
            provenance: &self.provenance,
        };
        driver.run(1, arm(configuration), runner)
    }

    /// The result of trial 1, which must have one.
    fn result(&self, configuration: Configuration, sandbox: &Scripted) -> Value {
        let path = self
            .run(configuration, sandbox, &Stub::new())
            .unwrap()
            .result();
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn trial_root(&self, configuration: Configuration) -> PathBuf {
        self.batch
            .eval_root()
            .join(format!("batch/trial-1-{configuration}"))
    }
}

#[test]
fn a_solved_trial_is_graded_and_recorded() {
    let bench = Bench::new();

    let result = bench.result(Configuration::Baseline, &Scripted::new(Session::Solves));

    assert_eq!(result["reached"]["phase"], "graded", "{result:#}");
    assert_eq!(result["outcome"]["result"], "succeeded", "{result:#}");
    assert_eq!(result["session"]["ended"]["status"], "completed");
    assert!(result["expectations"].is_null());
}

#[test]
fn a_symposium_trial_is_graded_with_its_expectations() {
    let bench = Bench::new();

    let result = bench.result(Configuration::Symposium, &Scripted::new(Session::Solves));

    assert_eq!(result["reached"]["phase"], "graded", "{result:#}");
    assert!(result["expectations"].is_object(), "{result:#}");
    // Nothing played Symposium's part, and the stub's transcript cannot show the
    // missing evidence was left alone, so no expectation is decided.
    assert_eq!(result["outcome"]["result"], "inconclusive", "{result:#}");
}

#[test]
fn a_failed_preparation_stops_at_setup() {
    let bench = Bench::new();
    let starter = bench.inputs.evals().starter_dir(bench.inputs.task().name());
    fs::remove_dir_all(starter).unwrap();

    let result = bench.result(Configuration::Baseline, &Scripted::new(Session::Solves));

    assert_eq!(result["reached"]["phase"], "setup", "{result:#}");
    assert_eq!(result["outcome"]["result"], "setup_failure");
    assert!(result["session"].is_null());
}

#[test]
fn a_failed_init_stops_at_setup() {
    let bench = Bench::new();
    let sandbox = Scripted {
        init: Init::Fails,
        ..Scripted::new(Session::Solves)
    };

    let result = bench.result(Configuration::Symposium, &sandbox);

    assert_eq!(result["reached"]["phase"], "setup", "{result:#}");
    assert!(
        result["reached"]["error"]
            .as_str()
            .unwrap()
            .contains("cargo agents init"),
        "{result:#}"
    );
    assert!(result["session"].is_null());
}

#[test]
fn a_failed_check_before_launch_stops_at_prelaunch() {
    let bench = Bench::new();
    let sandbox = Scripted {
        init: Init::Syncs,
        ..Scripted::new(Session::Solves)
    };

    let result = bench.result(Configuration::Symposium, &sandbox);

    assert_eq!(result["reached"]["phase"], "prelaunch", "{result:#}");
    assert_eq!(result["outcome"]["result"], "setup_failure");
    assert!(result["session"].is_null());
}

#[test]
fn a_client_that_cannot_start_leaves_no_session_to_keep() {
    let bench = Bench::new();
    let done = bench
        .run(
            Configuration::Baseline,
            &Scripted::new(Session::CannotStart),
            &Stub::new(),
        )
        .unwrap();
    let kept = bench
        .trial_root(Configuration::Baseline)
        .with_extension("kept");

    done.retain(&kept).unwrap();

    assert!(done.session.is_none(), "{done:?}");
    for absent in ["transcript.jsonl", "stderr.log", "response.txt"] {
        assert!(!kept.join(absent).exists(), "{absent}");
    }
    assert!(kept.join(RESULT_FILE).is_file());
}

#[test]
fn uncertain_client_containment_prevents_retention() {
    for session in [Session::Unsupervised, Session::UncleanTimeout] {
        let bench = Bench::new();
        let done = bench
            .run(
                Configuration::Baseline,
                &Scripted::new(session),
                &Stub::new(),
            )
            .unwrap();
        let kept = bench
            .trial_root(Configuration::Baseline)
            .with_extension("kept");

        let retained = done.retain(&kept);

        assert!(matches!(retained, Err(retain::RetainError::NotQuiescent)));
        assert!(!kept.exists());
    }
}

fn assert_retention_stops_before_inspection(done: Done) {
    let destination = done.root.with_extension("kept");
    let parent = destination.parent().unwrap();
    fs::remove_dir_all(&done.root).unwrap();
    let before: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();

    let retained = done.retain(&destination);

    assert!(matches!(retained, Err(retain::RetainError::NotQuiescent)));
    assert!(!destination.exists());
    let after: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(after, before, "retention left a staging artifact");
}

#[test]
fn uncertain_setup_timeout_prevents_retention_before_inspection() {
    let bench = Bench::new();
    let sandbox = Scripted {
        init: Init::UncleanTimeout,
        ..Scripted::new(Session::Solves)
    };
    let done = bench
        .run(Configuration::Symposium, &sandbox, &Stub::new())
        .unwrap();

    assert_retention_stops_before_inspection(done);
}

#[test]
fn uncertain_grading_timeout_prevents_retention_before_inspection() {
    let bench = Bench::new();
    let sandbox = Scripted {
        unclean_grading_timeout: true,
        ..Scripted::new(Session::Solves)
    };
    let done = bench
        .run(Configuration::Baseline, &sandbox, &Stub::new())
        .unwrap();

    assert_retention_stops_before_inspection(done);
}

#[test]
fn a_client_that_cannot_start_is_kept_and_not_graded() {
    let bench = Bench::new();

    let result = bench.result(
        Configuration::Baseline,
        &Scripted::new(Session::CannotStart),
    );

    assert_eq!(result["reached"]["phase"], "launch", "{result:#}");
    assert_eq!(result["session"]["ended"]["status"], "not_launched");
    assert!(result["code"].is_null());
    assert!(
        !bench
            .trial_root(Configuration::Baseline)
            .join("grading")
            .exists()
    );
}

#[test]
fn a_wrapper_exit_before_the_session_is_setup_failure_in_both_arms() {
    for code in [126, 127] {
        for configuration in [Configuration::Baseline, Configuration::Symposium] {
            let bench = Bench::new();
            let sandbox = Scripted::new(Session::WrapperExits(code));

            let result = bench.result(configuration, &sandbox);

            assert_eq!(result["reached"]["phase"], "launch", "{result:#}");
            assert_eq!(result["outcome"]["result"], "setup_failure");
            assert_eq!(result["session"]["ended"]["status"], "not_launched");
            assert!(
                result["session"]["ended"]["error"]
                    .as_str()
                    .is_some_and(|error| error.contains(&code.to_string())),
                "{result:#}"
            );
            assert!(result["code"].is_null(), "{result:#}");
            assert_eq!(sandbox.grading_calls.get(), 0);
            assert!(!bench.trial_root(configuration).join("grading").exists());
        }
    }
}

#[test]
fn a_timed_out_session_is_still_graded() {
    let bench = Bench::new();

    let result = bench.result(Configuration::Baseline, &Scripted::new(Session::TimesOut));

    assert_eq!(
        result["session"]["ended"]["status"], "timed_out",
        "{result:#}"
    );
    assert_eq!(result["reached"]["phase"], "graded");
    assert!(result["code"].is_object(), "{result:#}");
}

#[test]
fn a_trial_sandbox_that_cannot_be_prepared_stops_at_setup() {
    let bench = Bench::new();
    let sandbox = Scripted {
        trial_sandbox: false,
        ..Scripted::new(Session::Solves)
    };

    let result = bench.result(Configuration::Symposium, &sandbox);

    assert_eq!(result["reached"]["phase"], "setup", "{result:#}");
    assert_eq!(result["outcome"]["result"], "setup_failure");
    for phase in ["session", "code", "expectations"] {
        assert!(result[phase].is_null(), "{phase}: {result:#}");
    }
}

#[test]
fn a_grading_sandbox_that_cannot_be_prepared_excludes_the_trial() {
    let bench = Bench::new();
    let sandbox = Scripted {
        grading_sandbox: false,
        ..Scripted::new(Session::Solves)
    };

    let result = bench.result(Configuration::Baseline, &sandbox);

    assert_eq!(
        result["outcome"]["result"], "grading_exclusion",
        "{result:#}"
    );
}

#[test]
fn the_result_names_the_client_and_model_the_runner_launched() {
    let bench = Bench::new();
    let runner = Stub {
        client: Client::Codex,
        model: "another-model",
        ..Stub::new()
    };

    let path = bench
        .run(
            Configuration::Baseline,
            &Scripted::new(Session::CannotStart),
            &runner,
        )
        .unwrap()
        .result();

    let result: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(result["client"]["name"], "codex", "{result:#}");
    let session = bench.trial_root(Configuration::Baseline).join("session");
    let launched = fs::read_to_string(launched(&session)).unwrap();
    assert_eq!(launched, "another-model");
    assert_eq!(result["model"]["name"], launched, "{result:#}");
}

#[test]
fn a_planted_result_is_never_replaced() {
    let bench = Bench::new();

    let ran = bench.run(
        Configuration::Baseline,
        &Scripted::new(Session::PlantsResult),
        &Stub::new(),
    );

    assert!(matches!(ran, Err(TrialError::Write { .. })), "{ran:?}");
    let planted = bench.trial_root(Configuration::Baseline).join(RESULT_FILE);
    assert_eq!(fs::read_to_string(planted).unwrap(), "planted");
}

#[test]
fn parts_that_cannot_make_one_result_are_the_runners_error() {
    let bench = Bench::new();
    let runner = Stub {
        loses_loads: true,
        ..Stub::new()
    };

    let ran = bench.run(
        Configuration::Baseline,
        &Scripted::new(Session::Solves),
        &runner,
    );

    assert!(
        matches!(ran, Err(TrialError::Result(ResultError::Loads { .. }))),
        "{ran:?}"
    );
    let root = bench.trial_root(Configuration::Baseline);
    assert!(!root.join(RESULT_FILE).exists());
}

#[test]
fn a_trial_is_never_run_twice() {
    let bench = Bench::new();
    let sandbox = Scripted::new(Session::CannotStart);
    bench
        .run(Configuration::Baseline, &sandbox, &Stub::new())
        .unwrap();

    let again = bench.run(Configuration::Baseline, &sandbox, &Stub::new());

    assert!(matches!(again, Err(TrialError::Allocate(_))), "{again:?}");
}
