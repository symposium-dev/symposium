//! Each shape a result can take, pinned as a snapshot of its whole JSON. Hashes change
//! with `evals/` and the crate, so they are redacted.

mod refused;
mod written;

use std::io;
use std::path::Path;
use std::time::Duration;

use insta::assert_json_snapshot;

use super::*;
use crate::client::{
    ClientError, Ended, Execution, HookEvent, HookEvidence, Load, Summary, Tokens, ToolActivity,
    Usage,
};
use crate::expect::crosscheck::{Concern, Reason};
use crate::expect::decide::{Checked, Expectations, HookExpectation};
use crate::expect::installed::{InstallAssertion, InstallEvidence, SkillInstall};
use crate::expect::log::CallEvidence;
use crate::expect::sync::SyncEvidence;
use crate::expect::{SkillUse, UseAssertion, Verdict};
use crate::grade::{Grading, Outcome as Check, Report, Step, StepKind};
use crate::run::test_support::{self, Host};
use crate::task::{CheckName, ExpectedHook, HookName, SkillName};

pub(super) fn provenance() -> Provenance {
    Provenance {
        client_version: Some("2.1.316".to_owned()),
        os: "linux x86_64".to_owned(),
        toolchain: Some("rustc 1.99.1".to_owned()),
        isolation: IsolationMode::Enforced,
        srt_version: Some("0.0.81".to_owned()),
        deadlines: Deadlines {
            setup: Duration::from_secs(120),
            agent: Duration::from_secs(600),
            grading: Duration::from_secs(300),
        },
    }
}

pub(super) fn execution(ended: Ended, usage: Usage) -> Execution {
    Execution {
        transcript: "transcript.jsonl".into(),
        ended,
        loads: vec![Load::Loaded],
        hooks: HookEvidence::Unsupported,
        activity: ToolActivity::unread(),
        summary: Summary {
            model: Some("claude-haiku-5-5".to_owned()),
            usage,
            response: Some("done".to_owned()),
        },
    }
}

pub(super) fn passing() -> Grading {
    let step = |kind| Step {
        kind,
        outcome: Check::Passed,
    };
    Grading::Graded(Report {
        outside_editable: Vec::new(),
        steps: vec![
            step(StepKind::Build),
            step(StepKind::TestLib),
            step(StepKind::Hidden(CheckName::new("double").unwrap())),
            step(StepKind::Fmt),
        ],
    })
}

fn unconcerned<T>(decided: T) -> Checked<T> {
    Checked {
        decided,
        concerns: Vec::new(),
    }
}

/// Every expectation passing but the hook's, which a concern makes inconclusive.
pub(super) fn expectations() -> Expectations {
    let skill = SkillName::new("rust-formatting").unwrap();
    let hook = HookExpectation {
        hook: ExpectedHook {
            name: HookName::new("format-on-stop").unwrap(),
            event: HookEvent::Stop,
        },
        evidence: CallEvidence::Succeeded,
        records: 2,
    };
    let install = SkillInstall {
        skill: skill.clone(),
        assertion: InstallAssertion::Installed,
        evidence: InstallEvidence::Matching,
        verdict: Verdict::Pass,
    };
    Expectations {
        session_start: unconcerned(CallEvidence::Succeeded),
        sync: unconcerned(SyncEvidence::Recorded),
        hooks: vec![Checked {
            decided: hook,
            concerns: vec![Concern::Contradiction(Reason::ExtraHookRecords {
                event: HookEvent::Stop,
                records: 2,
                runs: 1,
            })],
        }],
        installs: vec![unconcerned(install)],
        uses: vec![SkillUse {
            skill,
            assertion: UseAssertion::Used,
            load: Load::Loaded,
            verdict: Verdict::Pass,
        }],
    }
}

fn arm(configuration: Configuration) -> Arm {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    match configuration {
        Configuration::Baseline => Arm::Baseline,
        Configuration::Symposium => Arm::Symposium(SymposiumBuild {
            version: "cargo-agents 1.0.0".to_owned(),
            sha256: crate::digest::file(&manifest).unwrap(),
            revision: None,
        }),
    }
}

/// The result, or why there is none, of a `formatting` trial in `configuration` that
/// got as far as `reached`.
pub(super) fn try_result_of(
    configuration: Configuration,
    reached: Reached<'_>,
) -> Result<TrialResult, ResultError> {
    let host = Host::new();
    let batch = host.batch();
    let inputs = test_support::inputs(&batch, "formatting");
    let planned = Planned {
        task: inputs.task(),
        content_hash: inputs.digest(),
        client: Client::ClaudeCode,
        model: "haiku",
        arm: arm(configuration),
        trial: 3,
    };
    TrialResult::new(planned, provenance(), reached)
}

pub(super) fn result_of(configuration: Configuration, reached: Reached<'_>) -> TrialResult {
    try_result_of(configuration, reached).unwrap()
}

pub(super) fn graded<'a>(
    execution: &'a Execution,
    grading: &'a Grading,
    expectations: Option<&'a Expectations>,
) -> Reached<'a> {
    Reached::Graded {
        execution,
        elapsed: Duration::from_millis(1500),
        grading,
        changes: &[],
        expectations,
    }
}

macro_rules! assert_result_snapshot {
    ($result:expr) => {
        assert_json_snapshot!($result, {
            ".task.content_hash" => "[hash]",
            ".symposium.sha256" => "[hash]",
        })
    };
}

#[test]
fn a_graded_symposium_trial() {
    let tokens = Tokens {
        input: 1,
        output: 2,
        cache_read: 3,
        cache_write: 4,
    };
    let execution = execution(Ended::Completed, Usage::Reported(tokens));
    let (grading, expectations) = (passing(), expectations());

    let result = result_of(
        Configuration::Symposium,
        graded(&execution, &grading, Some(&expectations)),
    );

    assert_result_snapshot!(result);
}

#[test]
fn a_trial_stopped_before_launch() {
    let error = io::Error::other("the policy is missing");
    let reached = Reached::Setup {
        step: SetupStep::Prelaunch,
        error: &error,
    };

    assert_result_snapshot!(result_of(Configuration::Symposium, reached));
}

#[test]
fn a_baseline_trial_whose_usage_is_unknown() {
    let execution = execution(Ended::Completed, Usage::Unknown);
    let grading = passing();

    let result = result_of(Configuration::Baseline, graded(&execution, &grading, None));

    assert_result_snapshot!(result);
}

#[test]
fn a_failed_session_on_a_client_without_usage() {
    let reported = ClientError::Reported("API Error: overloaded".to_owned());
    let execution = execution(Ended::Failed(reported), Usage::Unsupported);
    let grading = passing();

    let result = result_of(Configuration::Baseline, graded(&execution, &grading, None));

    assert_result_snapshot!(result);
}
