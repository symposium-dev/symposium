//! Deciding a Symposium trial's expectations, with every concern about the evidence
//! (RFD reference, "Expectations" and "What the evidence can and cannot show").
//!
//! [`gather`] reads what the trial left; [`decide`] reads nothing, so each rule can be
//! tested on its own facts. Each expectation keeps its evidence and its concerns apart,
//! as nextest keeps each attempt and derives "flaky" from them: the reported verdict is
//! derived, so a report shows both.

use std::path::{Path, PathBuf};

use super::config::ConfigChange;
use super::crosscheck::{self, Concern, Extent, Protected, ProtectedRoot};
use super::installed::{self, SkillInstall};
use super::log::{CallEvidence, Counted};
use super::sync::{self, SyncEvidence};
use super::{SkillUse, Verdict, hooks, trace};
use crate::client::{HookEvent, HookEvidence, InstalledSkill, ToolActivity};
use crate::run::client::Client;
use crate::run::{Configuration, Trial};
use crate::task::{ExpectedHook, SkillName, Task};

/// A trial of the Symposium configuration, the only one expectations apply to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SymposiumTrial<'a>(&'a Trial);

impl<'a> SymposiumTrial<'a> {
    /// `None` for a baseline trial, whose files say nothing about Symposium.
    pub(crate) fn new(trial: &'a Trial) -> Option<Self> {
        (trial.configuration() == Configuration::Symposium).then_some(Self(trial))
    }
}

/// Evidence that decides an expectation on its own.
pub(crate) trait HasVerdict {
    fn verdict(&self) -> Verdict;
}

impl HasVerdict for CallEvidence {
    fn verdict(&self) -> Verdict {
        CallEvidence::verdict(*self)
    }
}

impl HasVerdict for SyncEvidence {
    fn verdict(&self) -> Verdict {
        SyncEvidence::verdict(*self)
    }
}

impl HasVerdict for SkillInstall {
    fn verdict(&self) -> Verdict {
        self.verdict
    }
}

/// An expectation decided from its evidence, and every concern found about that
/// evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checked<T> {
    pub(crate) decided: T,
    pub(crate) concerns: Vec<Concern>,
}

impl<T: HasVerdict> Checked<T> {
    /// The verdict the evidence gives on its own.
    pub(crate) fn evidence_verdict(&self) -> Verdict {
        self.decided.verdict()
    }

    /// The verdict to report: any concern makes it inconclusive, never a pass or a fail.
    pub(crate) fn verdict(&self) -> Verdict {
        if self.concerns.is_empty() {
            self.evidence_verdict()
        } else {
            Verdict::Inconclusive
        }
    }
}

/// One `[expected] hooks` entry and what the hook records showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookExpectation {
    pub(crate) hook: ExpectedHook,
    pub(crate) evidence: CallEvidence,
    /// How many records the hook left, among those that can be read.
    pub(crate) records: usize,
}

impl HasVerdict for HookExpectation {
    fn verdict(&self) -> Verdict {
        self.evidence.verdict()
    }
}

/// What a Symposium trial left, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) session_start: CallEvidence,
    pub(crate) sync: SyncEvidence,
    pub(crate) hooks: Vec<HookExpectation>,
    pub(crate) installs: Vec<SkillInstall>,
}

/// What the transcript and config snapshot say, from outside the trial.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outside<'a> {
    pub(crate) hooks: &'a HookEvidence,
    pub(crate) activity: &'a ToolActivity,
    pub(crate) changes: &'a [ConfigChange],
}

/// Every expectation of a Symposium trial, decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expectations {
    pub(crate) session_start: Checked<CallEvidence>,
    pub(crate) sync: Checked<SyncEvidence>,
    pub(crate) hooks: Vec<Checked<HookExpectation>>,
    pub(crate) installs: Vec<Checked<SkillInstall>>,
    /// Decided from the transcript itself, so nothing can raise a concern about them.
    pub(crate) uses: Vec<SkillUse>,
}

impl Expectations {
    /// Every expectation's reported verdict, for the trial's outcome.
    pub(crate) fn verdicts(&self) -> Vec<Verdict> {
        let mut verdicts = vec![self.session_start.verdict(), self.sync.verdict()];
        verdicts.extend(self.hooks.iter().map(Checked::verdict));
        verdicts.extend(self.installs.iter().map(Checked::verdict));
        verdicts.extend(self.uses.iter().map(|used| used.verdict));
        verdicts
    }
}

/// Reads what `trial` left for `task` and `client`, with `source_of` giving each
/// skill's directory in `evals/`.
pub(crate) fn gather(
    SymposiumTrial(trial): SymposiumTrial<'_>,
    client: Client,
    task: &Task,
    source_of: impl Fn(&SkillName) -> Option<PathBuf>,
) -> Facts {
    let expected = task
        .expected_hooks()
        .iter()
        .map(|hook| HookExpectation {
            hook: hook.clone(),
            evidence: hooks::hook_record(&trial.hook_records(), &hook.name, counted(&hook.event)),
            records: hooks::record_count(&trial.hook_records(), &hook.name),
        })
        .collect();
    Facts {
        session_start: trace::session_start(&trial.hook_trace(), client.symposium_name()),
        sync: sync::sync_record(&trial.home(), &trial.sync_records(), &trial.project()),
        hooks: expected,
        installs: installed::skill_installs(
            &skills_dir(trial, client),
            task.expected_skills(),
            source_of,
        ),
    }
}

/// `Stop` can fire more than once, so its first record decides (RFD reference,
/// "Synchronization and hooks").
fn counted(event: &HookEvent) -> Counted {
    match event {
        HookEvent::Stop => Counted::First,
        HookEvent::SessionStart | HookEvent::Other(_) => Counted::Any,
    }
}

fn skills_dir(trial: &Trial, client: Client) -> PathBuf {
    trial.project().join(client.skill_dir())
}

/// Where `client` would install each skill `task` names, for the adapter to look for its
/// loads. A trial starts with no skills, and each name has one provider, so sync installs
/// it under its own name.
pub(crate) fn installed_skills(trial: &Trial, client: Client, task: &Task) -> Vec<InstalledSkill> {
    let skills = skills_dir(trial, client);
    task.expected_skills()
        .names()
        .map(|name| InstalledSkill {
            name: name.clone(),
            skill_md: skills.join(name.as_str()).join("SKILL.md"),
        })
        .collect()
}

/// Decides each expectation in `facts`, with the concerns `outside` raises about its
/// evidence.
pub(crate) fn decide(
    SymposiumTrial(trial): SymposiumTrial<'_>,
    client: Client,
    facts: Facts,
    outside: Outside<'_>,
    uses: Vec<SkillUse>,
) -> Expectations {
    let protected = |protected, extent, path: &Path| {
        let root = ProtectedRoot::new(trial.batch(), protected, extent, path);
        let mut concerns = crosscheck::activity(&root, outside.activity);
        concerns.extend(crosscheck::config(outside.changes));
        concerns
    };

    let mut concerns = crosscheck::session_start_trace(facts.session_start, outside.hooks);
    concerns.extend(protected(
        Protected::HookTrace,
        Extent::File,
        &trial.hook_trace(),
    ));
    let session_start = Checked {
        decided: facts.session_start,
        concerns,
    };

    let mut concerns = crosscheck::sync_record(facts.sync, outside.hooks);
    concerns.extend(protected(
        Protected::SyncRecords,
        Extent::Tree,
        &trial.sync_records(),
    ));
    let sync = Checked {
        decided: facts.sync,
        concerns,
    };

    let records = protected(Protected::HookRecords, Extent::File, &trial.hook_records());
    let hooks = facts
        .hooks
        .into_iter()
        .map(|expectation| {
            let mut concerns = crosscheck::plugin_hook_record(
                expectation.evidence,
                expectation.records,
                &expectation.hook.event,
                outside.hooks,
            );
            concerns.extend(records.iter().cloned());
            Checked {
                decided: expectation,
                concerns,
            }
        })
        .collect();

    let skills = protected(
        Protected::InstalledSkills,
        Extent::Tree,
        &skills_dir(trial, client),
    );
    let installs = facts
        .installs
        .into_iter()
        .map(|install| Checked {
            decided: install,
            concerns: skills.clone(),
        })
        .collect();

    Expectations {
        session_start,
        sync,
        hooks,
        installs,
        uses,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::client::{FileMutation, HookRun, HookRuns, ObservedPath, ToolResult};
    use crate::expect::config::{Change, Root};
    use crate::expect::crosscheck::Reason;
    use crate::expect::installed::{InstallAssertion, InstallEvidence};
    use crate::run::setup::SYMPOSIUM_CONFIG;
    use crate::run::test_support::{self, Host};
    use crate::task::HookName;

    const OBSERVED: &[HookEvent] = &[HookEvent::SessionStart, HookEvent::Stop];

    fn trial(host: &Host) -> Trial {
        test_support::trial(&host.batch(), 1, Configuration::Symposium)
    }

    fn symposium(trial: &Trial) -> SymposiumTrial<'_> {
        SymposiumTrial::new(trial).unwrap()
    }

    fn on_stop() -> ExpectedHook {
        ExpectedHook {
            name: HookName::new("on-stop").unwrap(),
            event: HookEvent::Stop,
        }
    }

    /// Facts whose evidence all passes.
    fn passing() -> Facts {
        Facts {
            session_start: CallEvidence::Succeeded,
            sync: SyncEvidence::Recorded,
            hooks: vec![HookExpectation {
                hook: on_stop(),
                evidence: CallEvidence::Succeeded,
                records: 1,
            }],
            installs: vec![SkillInstall {
                skill: SkillName::new("first-skill").unwrap(),
                assertion: InstallAssertion::Installed,
                evidence: InstallEvidence::Matching,
                verdict: Verdict::Pass,
            }],
        }
    }

    /// A complete transcript in which `SessionStart` and `Stop` each succeeded once.
    fn both_ran() -> HookEvidence {
        let ran = |event| HookRun {
            event,
            result: crate::client::HookResult::Succeeded { exit_code: Some(0) },
        };
        HookEvidence::Reported(HookRuns {
            runs: vec![ran(HookEvent::SessionStart), ran(HookEvent::Stop)],
            observed: OBSERVED,
            complete: true,
        })
    }

    fn quiet() -> ToolActivity {
        ToolActivity {
            mutations: Vec::new(),
            commands: Vec::new(),
            complete: true,
        }
    }

    fn wrote(path: &Path) -> ToolActivity {
        let mut activity = quiet();
        activity.mutations.push(FileMutation {
            path: ObservedPath::Readable {
                reported: path.to_string_lossy().into_owned(),
                resolved: Some(path.to_owned()),
            },
            result: ToolResult::Succeeded,
        });
        activity
    }

    fn decided(
        trial: &Trial,
        facts: Facts,
        hooks: &HookEvidence,
        activity: &ToolActivity,
        changes: &[ConfigChange],
    ) -> Expectations {
        let outside = Outside {
            hooks,
            activity,
            changes,
        };
        decide(
            symposium(trial),
            Client::ClaudeCode,
            facts,
            outside,
            Vec::new(),
        )
    }

    fn verdicts(expectations: &Expectations) -> [Verdict; 4] {
        [
            expectations.session_start.verdict(),
            expectations.sync.verdict(),
            expectations.hooks[0].verdict(),
            expectations.installs[0].verdict(),
        ]
    }

    #[test]
    fn a_hook_record_beyond_the_runs_makes_the_hook_inconclusive() {
        let host = Host::new();
        let trial = trial(&host);
        let mut facts = passing();
        // A record the agent wrote before the hook's own, through a script the transcript
        // does not show writing it.
        facts.hooks[0].records = 2;

        let expectations = decided(&trial, facts, &both_ran(), &quiet(), &[]);

        assert_eq!(
            verdicts(&expectations),
            [
                Verdict::Pass,
                Verdict::Pass,
                Verdict::Inconclusive,
                Verdict::Pass
            ]
        );
        assert_eq!(expectations.hooks[0].evidence_verdict(), Verdict::Pass);
    }

    #[test]
    fn a_baseline_trial_has_no_expectations_to_decide() {
        let host = Host::new();
        let baseline = test_support::trial(&host.batch(), 1, Configuration::Baseline);

        assert!(SymposiumTrial::new(&baseline).is_none());
    }

    #[test]
    fn every_expectation_gives_its_reported_verdict() {
        let host = Host::new();
        let trial = trial(&host);
        let mut facts = passing();
        facts.hooks[0].records = 2;

        let expectations = decided(&trial, facts, &both_ran(), &quiet(), &[]);

        assert_eq!(
            expectations.verdicts(),
            [
                Verdict::Pass,
                Verdict::Pass,
                Verdict::Inconclusive,
                Verdict::Pass
            ]
        );
    }

    #[test]
    fn evidence_nothing_contradicts_keeps_its_own_verdict() {
        let host = Host::new();
        let trial = trial(&host);

        let expectations = decided(&trial, passing(), &both_ran(), &quiet(), &[]);

        assert_eq!(verdicts(&expectations), [Verdict::Pass; 4]);
    }

    #[test]
    fn a_failure_with_no_concern_stays_a_failure() {
        let host = Host::new();
        let trial = trial(&host);
        let mut facts = passing();
        facts.session_start = CallEvidence::Missing;
        let hooks = HookEvidence::Reported(HookRuns {
            runs: Vec::new(),
            observed: OBSERVED,
            complete: true,
        });

        let expectations = decided(&trial, facts, &hooks, &quiet(), &[]);

        assert_eq!(expectations.session_start.verdict(), Verdict::Fail);
        assert_eq!(expectations.session_start.concerns, []);
    }

    #[test]
    fn a_concern_makes_a_pass_inconclusive_and_keeps_the_evidence_verdict() {
        let host = Host::new();
        let trial = trial(&host);
        let hooks = HookEvidence::Reported(HookRuns {
            runs: Vec::new(),
            observed: OBSERVED,
            complete: true,
        });

        let expectations = decided(&trial, passing(), &hooks, &quiet(), &[]);
        let session_start = &expectations.session_start;

        assert_eq!(session_start.evidence_verdict(), Verdict::Pass);
        assert_eq!(session_start.verdict(), Verdict::Inconclusive);
        assert_eq!(
            session_start.concerns,
            [Concern::Contradiction(Reason::HookAbsent {
                event: HookEvent::SessionStart
            })]
        );
    }

    #[test]
    fn a_write_bears_only_on_the_expectations_it_could_alter() {
        let host = Host::new();
        let trial = trial(&host);
        let skills = trial.project().join(Client::ClaudeCode.skill_dir());
        let cases = [
            (
                trial.hook_trace(),
                [
                    Verdict::Inconclusive,
                    Verdict::Pass,
                    Verdict::Pass,
                    Verdict::Pass,
                ],
            ),
            (
                trial.sync_records().join("x/state.json"),
                [
                    Verdict::Pass,
                    Verdict::Inconclusive,
                    Verdict::Pass,
                    Verdict::Pass,
                ],
            ),
            (
                trial.hook_records(),
                [
                    Verdict::Pass,
                    Verdict::Pass,
                    Verdict::Inconclusive,
                    Verdict::Pass,
                ],
            ),
            (
                skills.join("first-skill/SKILL.md"),
                [
                    Verdict::Pass,
                    Verdict::Pass,
                    Verdict::Pass,
                    Verdict::Inconclusive,
                ],
            ),
        ];
        for (path, expected) in cases {
            let expectations = decided(&trial, passing(), &both_ran(), &wrote(&path), &[]);

            assert_eq!(verdicts(&expectations), expected, "{}", path.display());
        }
    }

    #[test]
    fn any_config_change_bears_on_every_expectation() {
        let host = Host::new();
        let trial = trial(&host);
        let change = |file| ConfigChange {
            root: Root::Home,
            file,
            change: Change::Modified,
        };

        let client = decided(
            &trial,
            passing(),
            &both_ran(),
            &quiet(),
            &[change(".claude/settings.json")],
        );
        let symposium = decided(
            &trial,
            passing(),
            &both_ran(),
            &quiet(),
            &[change(SYMPOSIUM_CONFIG)],
        );

        assert_eq!(verdicts(&client), [Verdict::Inconclusive; 4]);
        assert_eq!(verdicts(&symposium), [Verdict::Inconclusive; 4]);
    }

    #[cfg(unix)]
    #[test]
    fn a_changed_client_hook_cannot_forge_a_passing_skill_install() {
        use crate::expect::config::Snapshot;

        let host = Host::new();
        let batch = host.batch();
        let inputs = test_support::inputs(&batch, "formatting");
        let task = inputs.task();
        let trial = batch
            .create_trial(1, Configuration::Symposium, &inputs)
            .unwrap();
        let snapshot = Snapshot::take(&trial.home(), &trial.project(), Client::ClaudeCode);
        let settings = trial.project().join(".claude/settings.local.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let command = concat!(
            "mkdir -p .claude/skills/rust-formatting && ",
            "cp ../registry/format-on-stop/skills/rust-formatting/SKILL.md ",
            ".claude/skills/rust-formatting/SKILL.md && ",
            "touch .claude/skills/rust-formatting/.symposium"
        );
        let config = serde_json::json!({
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": command}]}]
            }
        });
        fs::write(&settings, config.to_string()).unwrap();

        // A client hook runs outside the agent's tool calls, so only the settings
        // edit appears in tool activity. Execute its command without a model.
        let output = std::process::Command::new("sh")
            .args(["-c", command])
            .current_dir(trial.project())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");

        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evals/plugins/format-on-stop/skills/rust-formatting");
        let facts = gather(symposium(&trial), Client::ClaudeCode, task, |_| {
            Some(source.clone())
        });
        assert_eq!(facts.installs[0].evidence, InstallEvidence::Matching);
        let changes = snapshot.changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file, ".claude/settings.local.json");

        let expectations = decided(&trial, facts, &both_ran(), &wrote(&settings), &changes);

        assert_eq!(expectations.installs[0].evidence_verdict(), Verdict::Pass);
        assert_eq!(expectations.installs[0].verdict(), Verdict::Inconclusive);
    }

    #[test]
    fn each_named_skill_is_looked_for_where_sync_installs_it() {
        let host = Host::new();
        let batch = host.batch();
        let inputs = test_support::inputs(&batch, "formatting");
        let task = inputs.task();
        let trial = batch
            .create_trial(1, Configuration::Symposium, &inputs)
            .unwrap();

        let skills = installed_skills(&trial, Client::ClaudeCode, task);

        let skill_md = [".claude", "skills", "rust-formatting", "SKILL.md"]
            .iter()
            .fold(trial.project(), |path, part| path.join(part));
        assert_eq!(
            skills,
            [InstalledSkill {
                name: SkillName::new("rust-formatting").unwrap(),
                skill_md,
            }]
        );
    }

    #[test]
    fn a_stop_hook_is_gathered_by_its_first_record() {
        let host = Host::new();
        let batch = host.batch();
        let inputs = test_support::inputs(&batch, "formatting");
        let task = inputs.task();
        let trial = batch
            .create_trial(1, Configuration::Symposium, &inputs)
            .unwrap();
        let record = |exit_code: u8| {
            serde_json::json!({ "hook": "format-on-stop", "exit_code": exit_code }).to_string()
        };
        fs::write(
            trial.hook_records(),
            format!("{}\n{}\n", record(1), record(0)),
        )
        .unwrap();

        let facts = gather(symposium(&trial), Client::ClaudeCode, task, |_| None);

        assert_eq!(facts.hooks[0].hook.event, HookEvent::Stop);
        assert_eq!(facts.hooks[0].evidence, CallEvidence::Failed);
    }
}
